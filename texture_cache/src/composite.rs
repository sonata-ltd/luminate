//! Draws a cached texture as a textured quad through `iced_wgpu`'s custom
//! primitive API. The render pass viewport is already set to the primitive's
//! bounds, so one clip-space triangle that covers the viewport fills exactly
//! the composite rectangle.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use iced_core::Rectangle;
use iced_graphics::Viewport;
use iced_wgpu::primitive::{Pipeline, Primitive};

use crate::filter::FilterQuality;
use crate::warp::Warp;

const SHADER: &str = include_str!("shader/composite.wgsl");

/// Instances a single texture can be composited per frame before its params
/// buffer grows.
const INITIAL_INSTANCES: u32 = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    opacity: f32,
    /// The reconstruction kernel; see [`FilterQuality::shader_mode`].
    mode: f32,
    /// How far the genie's stretch has run. `0.0` is no warp, and the
    /// shader's fast path.
    warp_stretch: f32,
    /// How far its squash has run: the travel along the axis.
    warp_squash: f32,
    /// Axis mirrors that put the genie's anchor corner at the origin, as
    /// `0.0` or `1.0`: WGSL uniforms carry no booleans.
    warp_flip_x: f32,
    warp_flip_y: f32,
    _pad: [f32; 2],
}

const PARAMS_SIZE: u64 = std::mem::size_of::<Params>() as u64;
const _: () = assert!(PARAMS_SIZE == 32, "the WGSL `Params` struct is 32 bytes");

impl Params {
    fn new(opacity: f32, filter: FilterQuality, warp: Warp) -> Self {
        let (warp_stretch, warp_squash, warp_flip_x, warp_flip_y) = match warp {
            Warp::None => (0.0, 0.0, 0.0, 0.0),
            Warp::Genie(genie) => {
                let (stretch, squash) = genie.phases();
                let (flip_x, flip_y) = genie.flips();
                (
                    stretch,
                    squash,
                    f32::from(u8::from(flip_x)),
                    f32::from(u8::from(flip_y)),
                )
            }
        };

        Self {
            opacity,
            mode: filter.shader_mode(),
            warp_stretch,
            warp_squash,
            warp_flip_x,
            warp_flip_y,
            _pad: [0.0; 2],
        }
    }
}

/// One composite of a cached texture into the frame.
#[derive(Debug)]
pub(crate) struct CompositePrimitive {
    view: Arc<wgpu::TextureView>,
    /// Group opacity, already normalised to `0.0..=1.0` by
    /// `TextureRenderer::draw_cached`.
    opacity: f32,
    /// The reconstruction kernel this composite uses. Per instance, not per
    /// pipeline: two widgets sharing a texture may ask for different tiers.
    filter: FilterQuality,
    /// The warp this composite applies. Per instance, like `opacity` and
    /// `filter`: two widgets may share a texture and warp differently.
    warp: Warp,
    /// The instance `prepare` assigned, read back by `draw`. Stored on the
    /// primitive so `draw` does not depend on being called in preparation
    /// order.
    instance: AtomicU32,
}

impl CompositePrimitive {
    pub(crate) fn new(
        view: Arc<wgpu::TextureView>,
        opacity: f32,
        filter: FilterQuality,
        warp: Warp,
    ) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity is normalised before a primitive is built"
        );

        Self {
            view,
            opacity,
            filter,
            warp,
            instance: AtomicU32::new(0),
        }
    }
}

/// Identity of a texture: the address of its shared view. A binding holds a
/// clone of the `Arc`, so the address cannot be reused while the binding
/// exists.
fn texture_key<T>(view: &Arc<T>) -> usize {
    Arc::as_ptr(view).addr()
}

/// Per-texture GPU state of the composite pipeline.
///
/// The same texture may be composited several times per frame (a clone of
/// the cache elsewhere in the tree, at another opacity), so `params` holds
/// one uniform block per instance, addressed by a dynamic offset. `prepare`
/// hands out instances and records them on the primitive; `shadow` mirrors
/// the buffer so it can be re-uploaded whole when the buffer grows.
struct Binding {
    view: Arc<wgpu::TextureView>,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: u32,
    shadow: Vec<Params>,
}

/// Appends one instance's parameters to `shadow` and returns its instance.
fn assign_instance(
    shadow: &mut Vec<Params>,
    opacity: f32,
    filter: FilterQuality,
    warp: Warp,
) -> u32 {
    shadow.push(Params::new(opacity, filter, warp));
    u32::try_from(shadow.len() - 1).expect("fewer than u32::MAX composites per frame")
}

/// Shared GPU pipeline for all [`CompositePrimitive`]s. Created once by
/// `iced_wgpu` via [`Pipeline::new`].
pub(crate) struct CompositePipeline {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    /// Byte stride between instances in a binding's params buffer.
    stride: u64,
    /// Keyed by [`texture_key`].
    bindings: HashMap<usize, Binding>,
}

impl CompositePipeline {
    fn create_binding(
        &self,
        device: &wgpu::Device,
        view: Arc<wgpu::TextureView>,
        capacity: u32,
    ) -> Binding {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iced_texture_cache composite params"),
            size: self.stride * u64::from(capacity),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_texture_cache composite bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &params,
                        offset: 0,
                        size: wgpu::BufferSize::new(PARAMS_SIZE),
                    }),
                },
            ],
        });

        Binding {
            view,
            params,
            bind_group,
            capacity,
            shadow: Vec::with_capacity(capacity as usize),
        }
    }

    /// Uploads every instance of `binding` in one write (used after the
    /// buffer was replaced).
    fn upload_all(&self, queue: &wgpu::Queue, binding: &Binding) {
        let stride = self.stride as usize;
        let mut bytes = vec![0u8; stride * binding.shadow.len()];

        for (chunk, params) in bytes.chunks_exact_mut(stride).zip(&binding.shadow) {
            chunk[..PARAMS_SIZE as usize].copy_from_slice(bytemuck::bytes_of(params));
        }

        queue.write_buffer(&binding.params, 0, &bytes);
    }
}

impl Pipeline for CompositePipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_texture_cache composite shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_texture_cache composite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(PARAMS_SIZE),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iced_texture_cache composite pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iced_texture_cache composite pipeline"),
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
                    // iced_wgpu renders premultiplied output, so composite premultiplied.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // No mip chain: minified composites (`Cached::scale` well below 1)
        // alias; documented in the README's limitations.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iced_texture_cache composite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });

        let stride =
            u64::from(device.limits().min_uniform_buffer_offset_alignment).max(PARAMS_SIZE);

        Self {
            pipeline,
            sampler,
            layout,
            stride,
            bindings: HashMap::new(),
        }
    }

    fn trim(&mut self) {
        // A binding is kept while either the store's entry or a primitive
        // recorded this frame still references the view; once only our
        // reference remains the cache is gone. Instances restart for the
        // next `present`.
        self.bindings.retain(|_, binding| {
            binding.shadow.clear();
            Arc::strong_count(&binding.view) > 1
        });
    }
}

impl Primitive for CompositePrimitive {
    type Pipeline = CompositePipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        let key = texture_key(&self.view);

        if !pipeline.bindings.contains_key(&key) {
            let binding = pipeline.create_binding(device, self.view.clone(), INITIAL_INSTANCES);
            let _ = pipeline.bindings.insert(key, binding);
        }

        let needs_growth = pipeline
            .bindings
            .get(&key)
            .is_some_and(|binding| binding.shadow.len() as u32 >= binding.capacity);

        if needs_growth {
            // Instances already handed out this frame keep their slots: the
            // shadow copy is uploaded whole into the larger buffer.
            let Some(old) = pipeline.bindings.remove(&key) else {
                return;
            };
            let mut binding = pipeline.create_binding(device, self.view.clone(), old.capacity * 2);
            binding.shadow = old.shadow;
            pipeline.upload_all(queue, &binding);
            let _ = pipeline.bindings.insert(key, binding);
        }

        let Some(binding) = pipeline.bindings.get_mut(&key) else {
            return;
        };

        let index = assign_instance(&mut binding.shadow, self.opacity, self.filter, self.warp);
        queue.write_buffer(
            &binding.params,
            pipeline.stride * u64::from(index),
            bytemuck::bytes_of(&binding.shadow[index as usize]),
        );
        self.instance.store(index, Ordering::Relaxed);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(binding) = pipeline.bindings.get(&texture_key(&self.view)) else {
            return false;
        };

        let index = self.instance.load(Ordering::Relaxed);
        if index as usize >= binding.shadow.len() {
            return false;
        }

        let offset = u32::try_from(pipeline.stride * u64::from(index)).unwrap_or(u32::MAX);

        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &binding.bind_group, &[offset]);
        render_pass.draw(0..3, 0..1);

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::{Corner, Genie};

    #[test]
    fn an_absent_warp_takes_the_shaders_identity_path() {
        let params = Params::new(1.0, FilterQuality::Bilinear, Warp::None);
        assert_eq!(
            (params.warp_stretch, params.warp_squash),
            (0.0, 0.0),
            "the shader's identity path needs both phases at zero"
        );
    }

    #[test]
    fn a_fully_open_genie_is_indistinguishable_from_no_warp() {
        let none = Params::new(1.0, FilterQuality::Bilinear, Warp::None);
        let open = Params::new(
            1.0,
            FilterQuality::Bilinear,
            Warp::Genie(Genie::new(1.0, Corner::TopLeft)),
        );
        assert_eq!(none.warp_stretch, open.warp_stretch);
        assert_eq!(none.warp_squash, open.warp_squash);
    }

    #[test]
    fn instances_keep_their_slot_and_opacity_across_growth() {
        let filter = FilterQuality::CatmullRom;
        let mut shadow = Vec::new();
        let a = assign_instance(&mut shadow, 0.25, filter, Warp::None);
        let b = assign_instance(&mut shadow, 0.5, filter, Warp::None);
        // "Growth": the shadow moves to a new binding unchanged.
        let moved = shadow;
        let mut grown = moved.clone();
        let c = assign_instance(&mut grown, 1.0, filter, Warp::None);
        assert_eq!((a, b, c), (0, 1, 2));
        assert_eq!(moved[a as usize].opacity, 0.25);
        assert_eq!(moved[b as usize].opacity, 0.5);
    }

    #[test]
    fn instances_of_one_texture_carry_their_own_filter() {
        // The same cache composited twice in a frame at two tiers: each
        // instance's uniform block must keep the tier it was assigned.
        let mut shadow = Vec::new();
        let sharp = assign_instance(&mut shadow, 1.0, FilterQuality::CatmullRom, Warp::None);
        let cheap = assign_instance(&mut shadow, 1.0, FilterQuality::Bilinear, Warp::None);
        let snapped = assign_instance(&mut shadow, 1.0, FilterQuality::Snap, Warp::None);

        assert_eq!(shadow[sharp as usize].mode, 0.0);
        assert_eq!(shadow[cheap as usize].mode, 1.0);
        // `Snap` shares the single-tap path; the geometry is what differs.
        assert_eq!(shadow[snapped as usize].mode, 1.0);
    }

    #[test]
    fn bindings_are_keyed_by_texture_identity_not_cache_id() {
        // Two textures recorded for the same cache in one frame (two `Cached`
        // widgets sharing a `TextureCache` at different sizes) must not share
        // a binding; two handles to one texture must.
        let first = Arc::new(0u8);
        let second = Arc::new(0u8);
        let first_again = Arc::clone(&first);
        assert_ne!(texture_key(&first), texture_key(&second));
        assert_eq!(texture_key(&first), texture_key(&first_again));
    }

    #[test]
    fn the_shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        let _ = validator.validate(&module).expect("WGSL validates");
    }
}
