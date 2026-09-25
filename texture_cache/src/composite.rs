//! Draws a cached texture as a textured quad through `iced_wgpu`'s custom
//! primitive API. The render pass viewport is already set to the primitive's
//! bounds, so one clip-space triangle that covers the viewport fills exactly
//! the composite rectangle.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use iced_core::{Rectangle, Size, border};
use iced_graphics::Viewport;
use iced_wgpu::primitive::{Pipeline, Primitive};

use crate::filter::FilterQuality;
use crate::geometry::clamp_radii;
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
    /// Device pixels per logical pixel. Both corner masks ramp across one
    /// device pixel, so an arc is as crisp on a dense display as on a 1x one.
    scale: f32,
    /// How far the genie's stretch has run. `0.0`, with `warp_squash` at
    /// `0.0`, is no warp and the shader's fast path.
    warp_stretch: f32,
    /// Corners of the composited rectangle, in the same logical pixels as
    /// `size`: `[top_left, top_right, bottom_right, bottom_left]`, iced's
    /// own order, already clamped so that no two on one side overlap. All
    /// zero is a rectangle and takes the shader's fast path.
    radius: [f32; 4],
    /// The size of the **shape** being cut, in logical pixels — the space
    /// the radii are measured in.
    size: [f32; 2],
    /// Where the drawn quad starts inside that shape, and how big it is.
    ///
    /// Usually `(0, 0)` and `size`: the quad is the shape. A pane of frosted
    /// glass that overhangs its source is the exception — it draws only the
    /// part that lies over the source, while its corners belong to the whole
    /// pane, so the coverage has to be sampled in the pane's coordinates
    /// rather than the drawn quad's.
    origin: [f32; 2],
    drawn: [f32; 2],
    /// How far the genie's squash has run: the travel along the axis.
    warp_squash: f32,
    /// The width of the band the rows converge on, as a fraction of the
    /// content's own width.
    warp_target_width: f32,
    /// The window on the texture this instance samples, in normalised
    /// coordinates: `[x, y, width, height]`.
    ///
    /// A plain composite takes the whole texture. A pane of frosted glass
    /// takes the part of its source that lies under it on screen, which is
    /// the whole of the backdrop semantics: moving the glass changes this
    /// rectangle and nothing else, so nothing is re-blurred.
    uv: [f32; 4],
    /// The content's corner radii in device pixels, in anchor space (see
    /// [`Genie::anchor_corners`](crate::warp::Genie::anchor_corners)). The
    /// genie's mask needs real pixels: a radius in `uv` would squeeze along
    /// with the shape, which is the whole thing it exists to avoid.
    warp_radius: [f32; 4],
    /// How sharply a row's travel lags with its distance from the anchor.
    warp_stretch_power: f32,
    /// The side curve's control values, at the wide end and the neck end.
    warp_curve_in: f32,
    warp_curve_out: f32,
    /// Axis mirrors that put the genie's anchor corner at the origin, as
    /// `0.0` or `1.0`: WGSL uniforms carry no booleans.
    warp_flip_x: f32,
    warp_flip_y: f32,
    /// The content rectangle's size in device pixels, the units of
    /// `warp_radius`.
    warp_rect_width: f32,
    warp_rect_height: f32,
    /// How far from the anchor the last row is drawn, as a fraction of the
    /// content's height; see [`Genie::far_edge`](crate::Genie::far_edge).
    warp_far_edge: f32,
    /// Where the content rectangle sits inside the texture, in `uv`: its
    /// origin and its size. The texture carries padding around the content
    /// and the warp is defined on the content, so the shader maps into this
    /// frame before it warps and back out to sample.
    warp_inset_x: f32,
    warp_inset_y: f32,
    warp_span_x: f32,
    warp_span_y: f32,
}

const PARAMS_SIZE: u64 = std::mem::size_of::<Params>() as u64;
const _: () = assert!(PARAMS_SIZE == 144, "the WGSL `Params` struct is 144 bytes");

/// Where an instance reads its texture and the rounded rectangle it cuts
/// out of its quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mask {
    /// The window on the texture this instance samples, in normalised
    /// coordinates. A plain composite passes the whole texture; a pane of
    /// frosted glass passes the part of its source that lies under it.
    pub(crate) uv: Rectangle,
    /// Corners of `shape`, in logical pixels.
    pub(crate) corners: border::Radius,
    /// The shape `corners` is measured in, as an offset and a size relative
    /// to the drawn quad's own origin.
    pub(crate) shape: Rectangle,
    /// The drawn quad's size in logical pixels.
    pub(crate) drawn: Size,
}

impl Mask {
    /// The whole texture, drawn over a quad of `size` that is its own shape.
    pub(crate) fn whole(size: Size, corners: border::Radius) -> Self {
        Self {
            uv: Rectangle::new(iced_core::Point::ORIGIN, Size::UNIT),
            corners,
            shape: Rectangle::with_size(size),
            drawn: size,
        }
    }
}

/// The rectangle a warp is defined on, as the shader needs it: the
/// content's size and corners in device pixels and where it sits inside the
/// texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// The content's size in device pixels.
    width: f32,
    height: f32,
    /// Device pixels per logical pixel.
    scale: f32,
    /// The content's corners in device pixels, iced's order, clamped so no
    /// two on one side overlap.
    corners: [f32; 4],
    /// The content's origin and size inside the texture, in `uv`.
    inset: (f32, f32),
    span: (f32, f32),
}

impl Frame {
    /// The frame for `content`, rounded to `corners`, composited inside
    /// `bounds`: all in logical pixels of the same space, on a renderer at
    /// `scale`.
    pub(crate) fn new(
        bounds: Rectangle,
        content: Rectangle,
        corners: border::Radius,
        scale: f32,
    ) -> Self {
        // A degenerate texture has nothing to warp; the identity frame
        // keeps the shader's arithmetic finite.
        let (inset, span) = if bounds.width > 0.0 && bounds.height > 0.0 {
            (
                (
                    (content.x - bounds.x) / bounds.width,
                    (content.y - bounds.y) / bounds.height,
                ),
                (content.width / bounds.width, content.height / bounds.height),
            )
        } else {
            ((0.0, 0.0), (1.0, 1.0))
        };

        Self {
            width: content.width * scale,
            height: content.height * scale,
            scale,
            corners: clamp_radii(content.size(), corners.into()).map(|radius| radius * scale),
            inset,
            span,
        }
    }
}

/// The warp's half of [`Params`], flattened for the uniform.
///
/// A struct rather than a ten-tuple so the fields are named at the one place
/// a `Warp::None` has to agree with a fully open genie.
struct WarpParams {
    stretch: f32,
    squash: f32,
    target_width: f32,
    stretch_power: f32,
    curve_in: f32,
    curve_out: f32,
    flip_x: f32,
    flip_y: f32,
    radius: [f32; 4],
    far_edge: f32,
}

impl Default for WarpParams {
    fn default() -> Self {
        Self {
            stretch: 0.0,
            squash: 0.0,
            target_width: 0.0,
            stretch_power: 0.0,
            curve_in: 0.0,
            curve_out: 0.0,
            flip_x: 0.0,
            flip_y: 0.0,
            radius: [0.0; 4],
            // Fully open, every row is drawn: the far edge is the content's.
            far_edge: 1.0,
        }
    }
}

impl Params {
    fn new(opacity: f32, filter: FilterQuality, mask: Mask, warp: Warp, frame: Frame) -> Self {
        let warp = match warp {
            Warp::None => WarpParams::default(),
            Warp::Genie(genie) => {
                let (stretch, squash) = genie.phases();
                let (flip_x, flip_y) = genie.flips();
                let shape = genie.shape();
                WarpParams {
                    stretch,
                    squash,
                    target_width: shape.target_width,
                    stretch_power: shape.stretch_power,
                    curve_in: shape.curve_in,
                    curve_out: shape.curve_out,
                    flip_x: f32::from(u8::from(flip_x)),
                    flip_y: f32::from(u8::from(flip_y)),
                    radius: genie.anchor_corners(frame.corners),
                    far_edge: genie.far_edge(),
                }
            }
        };

        Self {
            opacity,
            mode: filter.shader_mode(),
            scale: frame.scale,
            warp_stretch: warp.stretch,
            // Clamped here as the software backend clamps them: unclamped,
            // a radius past half the shape turns the distance field inside
            // out and the whole composite vanishes on the GPU alone.
            radius: clamp_radii(mask.shape.size(), mask.corners.into()),
            size: [mask.shape.width, mask.shape.height],
            origin: [mask.shape.x, mask.shape.y],
            drawn: [mask.drawn.width, mask.drawn.height],
            warp_squash: warp.squash,
            warp_target_width: warp.target_width,
            uv: [mask.uv.x, mask.uv.y, mask.uv.width, mask.uv.height],
            warp_radius: warp.radius,
            warp_stretch_power: warp.stretch_power,
            warp_curve_in: warp.curve_in,
            warp_curve_out: warp.curve_out,
            warp_flip_x: warp.flip_x,
            warp_flip_y: warp.flip_y,
            warp_rect_width: frame.width,
            warp_rect_height: frame.height,
            warp_far_edge: warp.far_edge,
            warp_inset_x: frame.inset.0,
            warp_inset_y: frame.inset.1,
            warp_span_x: frame.span.0,
            warp_span_y: frame.span.1,
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
    /// The window on the texture and the shape cut out of the quad. Per
    /// instance for the same reason `filter` is: two widgets sharing a
    /// texture may want different shapes.
    mask: Mask,
    /// The warp this composite applies. Per instance, like `opacity` and
    /// `filter`: two widgets may share a texture and warp differently.
    warp: Warp,
    /// The content rectangle the warp is defined on: its size and corners
    /// in device pixels, which the genie's mask needs to keep its radius in
    /// real pixels rather than in `uv`, and where it sits inside the padded
    /// texture.
    frame: Frame,
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
        mask: Mask,
        warp: Warp,
        frame: Frame,
    ) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity is normalised before a primitive is built"
        );

        Self {
            view,
            opacity,
            filter,
            mask,
            warp,
            frame,
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
    mask: Mask,
    warp: Warp,
    frame: Frame,
) -> u32 {
    shadow.push(Params::new(opacity, filter, mask, warp, frame));
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

        let index = assign_instance(
            &mut binding.shadow,
            self.opacity,
            self.filter,
            self.mask,
            self.warp,
            self.frame,
        );
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
    use crate::warp::{Corner, Genie, GenieShape};
    use iced_core::{Point, Size};

    /// A nominal content rectangle at 1:1, alone in its texture, with
    /// square corners.
    const RECT: Frame = Frame {
        width: 100.0,
        height: 200.0,
        scale: 1.0,
        corners: [0.0; 4],
        inset: (0.0, 0.0),
        span: (1.0, 1.0),
    };

    /// A square quad over the whole texture, with square corners: what the
    /// tests that are not about the mask use.
    fn square() -> Mask {
        Mask::whole(Size::new(10.0, 10.0), border::Radius::default())
    }

    fn params(warp: Warp, frame: Frame) -> Params {
        Params::new(1.0, FilterQuality::Bilinear, square(), warp, frame)
    }

    #[test]
    fn the_genies_corners_are_uploaded_in_device_pixels() {
        // Documented in logical pixels, like every other size in the API;
        // the mask rounds in device pixels, like the rectangle it is given.
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 200.0));
        let at_2x = params(
            Warp::Genie(Genie::new(0.5, GenieShape::default())),
            Frame::new(bounds, bounds, border::Radius::from(8.0), 2.0),
        );
        assert_eq!(at_2x.warp_radius, [16.0; 4]);
        assert!((at_2x.warp_rect_width - 200.0).abs() < f32::EPSILON);
        assert!((at_2x.warp_rect_height - 400.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_genies_corners_are_reordered_into_anchor_space() {
        // Only the content's top-right corner is round. Collapsing into the
        // bottom-right corner mirrors the rows, so in anchor space — where
        // the anchor corner is the top-left — it is the bottom-left one.
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 200.0));
        let corners = border::Radius {
            top_right: 6.0,
            ..border::Radius::default()
        };
        let shape = GenieShape {
            anchor: Corner::BottomRight,
            ..GenieShape::default()
        };
        let uploaded = params(
            Warp::Genie(Genie::new(0.5, shape)),
            Frame::new(bounds, bounds, corners, 1.0),
        );
        assert_eq!(uploaded.warp_radius, [0.0, 0.0, 0.0, 6.0]);
    }

    #[test]
    fn corners_too_large_for_the_shape_are_clamped_before_upload() {
        // Two radii on one side cannot exceed that side. The software mask
        // clamped them, the shader did not, and a pill-shaped radius made
        // the whole composite transparent on the GPU alone.
        let pill = Mask::whole(Size::new(40.0, 20.0), border::Radius::from(1000.0));
        let uploaded = Params::new(1.0, FilterQuality::Bilinear, pill, Warp::None, RECT);
        assert_eq!(uploaded.radius, [10.0; 4]);

        let bounds = Rectangle::new(Point::ORIGIN, Size::new(40.0, 20.0));
        let frame = Frame::new(bounds, bounds, border::Radius::from(1000.0), 1.0);
        assert_eq!(frame.corners, [10.0; 4]);
    }

    #[test]
    fn the_scale_is_uploaded_for_a_device_pixel_ramp() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(10.0, 10.0));
        let at_2x = params(
            Warp::None,
            Frame::new(bounds, bounds, border::Radius::default(), 2.0),
        );
        assert!((at_2x.scale - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_frame_places_the_content_inside_its_padded_texture() {
        // Two logical pixels of bleed on every side of a 10x20 content, on
        // a texture one pixel wider than that on the right.
        let bounds = Rectangle::new(Point::new(8.0, 18.0), Size::new(15.0, 24.0));
        let content = Rectangle::new(Point::new(10.0, 20.0), Size::new(10.0, 20.0));
        let frame = Frame::new(bounds, content, border::Radius::default(), 1.0);
        assert!((frame.inset.0 - 2.0 / 15.0).abs() < 1e-6);
        assert!((frame.inset.1 - 2.0 / 24.0).abs() < 1e-6);
        assert!((frame.span.0 - 10.0 / 15.0).abs() < 1e-6);
        assert!((frame.span.1 - 20.0 / 24.0).abs() < 1e-6);
        assert!((frame.width - 10.0).abs() < f32::EPSILON);
        assert!((frame.height - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_far_edge_is_uploaded_and_open_where_there_is_no_warp() {
        let none = params(Warp::None, RECT);
        assert!((none.warp_far_edge - 1.0).abs() < f32::EPSILON);
        let genie = Genie::new(0.25, GenieShape::default());
        let live = params(Warp::Genie(genie), RECT);
        assert!((live.warp_far_edge - genie.far_edge()).abs() < f32::EPSILON);
        assert!(live.warp_far_edge < 1.0);
    }

    #[test]
    fn an_absent_warp_takes_the_shaders_identity_path() {
        let params = params(Warp::None, RECT);
        assert_eq!(
            (params.warp_stretch, params.warp_squash),
            (0.0, 0.0),
            "the shader's identity path needs both phases at zero"
        );
    }

    #[test]
    fn a_fully_open_genie_is_indistinguishable_from_no_warp() {
        let none = params(Warp::None, RECT);
        let open = params(Warp::Genie(Genie::new(1.0, GenieShape::default())), RECT);
        assert_eq!(none.warp_stretch, open.warp_stretch);
        assert_eq!(none.warp_squash, open.warp_squash);
    }

    fn assign(shadow: &mut Vec<Params>, opacity: f32, filter: FilterQuality) -> u32 {
        assign_instance(shadow, opacity, filter, square(), Warp::None, RECT)
    }

    #[test]
    fn instances_keep_their_slot_and_opacity_across_growth() {
        let filter = FilterQuality::CatmullRom;
        let mut shadow = Vec::new();
        let a = assign(&mut shadow, 0.25, filter);
        let b = assign(&mut shadow, 0.5, filter);
        // "Growth": the shadow moves to a new binding unchanged.
        let moved = shadow;
        let mut grown = moved.clone();
        let c = assign(&mut grown, 1.0, filter);
        assert_eq!((a, b, c), (0, 1, 2));
        assert_eq!(moved[a as usize].opacity, 0.25);
        assert_eq!(moved[b as usize].opacity, 0.5);
    }

    #[test]
    fn instances_of_one_texture_carry_their_own_filter() {
        // The same cache composited twice in a frame at two tiers: each
        // instance's uniform block must keep the tier it was assigned.
        let mut shadow = Vec::new();
        let sharp = assign(&mut shadow, 1.0, FilterQuality::CatmullRom);
        let cheap = assign(&mut shadow, 1.0, FilterQuality::Bilinear);
        let snapped = assign(&mut shadow, 1.0, FilterQuality::Snap);

        assert_eq!(shadow[sharp as usize].mode, 0.0);
        assert_eq!(shadow[cheap as usize].mode, 1.0);
        // `Snap` shares the single-tap path; the geometry is what differs.
        assert_eq!(shadow[snapped as usize].mode, 1.0);
    }

    #[test]
    fn an_instance_carries_its_own_window_on_the_texture() {
        // A pane of glass reads the part of its source that lies under it;
        // a plain composite reads the whole texture. Both may be instances
        // of the same texture in one frame.
        let mut shadow = Vec::new();
        let whole = assign(&mut shadow, 1.0, FilterQuality::Bilinear);
        let window = Mask {
            uv: Rectangle {
                x: 0.25,
                y: 0.5,
                width: 0.5,
                height: 0.25,
            },
            ..square()
        };
        let part = assign_instance(
            &mut shadow,
            1.0,
            FilterQuality::Bilinear,
            window,
            Warp::None,
            RECT,
        );

        assert_eq!(shadow[whole as usize].uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(shadow[part as usize].uv, [0.25, 0.5, 0.5, 0.25]);
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

    /// The two halves of the uniform block agree field for field. A
    /// mismatch compiles on both sides and shows only as garbage on a GPU,
    /// so the offsets are read back from the parsed shader.
    #[test]
    fn the_shader_reads_every_field_where_the_rust_side_writes_it() {
        use std::mem::offset_of;

        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        let (members, span) = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, span }
                    if ty.name.as_deref() == Some("Params") =>
                {
                    Some((members.clone(), *span))
                }
                _ => None,
            })
            .expect("the shader declares `Params`");

        let rust = [
            ("opacity", offset_of!(Params, opacity)),
            ("mode", offset_of!(Params, mode)),
            ("scale", offset_of!(Params, scale)),
            ("warp_stretch", offset_of!(Params, warp_stretch)),
            ("radius", offset_of!(Params, radius)),
            ("size", offset_of!(Params, size)),
            ("origin", offset_of!(Params, origin)),
            ("drawn", offset_of!(Params, drawn)),
            ("warp_squash", offset_of!(Params, warp_squash)),
            ("warp_target_width", offset_of!(Params, warp_target_width)),
            ("uv", offset_of!(Params, uv)),
            ("warp_radius", offset_of!(Params, warp_radius)),
            ("warp_stretch_power", offset_of!(Params, warp_stretch_power)),
            ("warp_curve_in", offset_of!(Params, warp_curve_in)),
            ("warp_curve_out", offset_of!(Params, warp_curve_out)),
            ("warp_flip_x", offset_of!(Params, warp_flip_x)),
            ("warp_flip_y", offset_of!(Params, warp_flip_y)),
            ("warp_rect_width", offset_of!(Params, warp_rect_width)),
            ("warp_rect_height", offset_of!(Params, warp_rect_height)),
            ("warp_far_edge", offset_of!(Params, warp_far_edge)),
            ("warp_inset_x", offset_of!(Params, warp_inset_x)),
            ("warp_inset_y", offset_of!(Params, warp_inset_y)),
            ("warp_span_x", offset_of!(Params, warp_span_x)),
            ("warp_span_y", offset_of!(Params, warp_span_y)),
        ];

        assert_eq!(members.len(), rust.len(), "the same number of fields");
        for (member, (name, offset)) in members.iter().zip(rust) {
            assert_eq!(
                member.name.as_deref(),
                Some(name),
                "fields in the same order"
            );
            assert_eq!(
                member.offset as usize, offset,
                "`{name}` at the same offset"
            );
        }
        assert_eq!(u64::from(span), PARAMS_SIZE, "the same size");
    }
}
