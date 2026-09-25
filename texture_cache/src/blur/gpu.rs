//! The GPU blur chain: halve to the target level, a separable Gaussian
//! there, and a pool of intermediate targets.
//!
//! Everything runs in its own command encoder, submitted by
//! [`BlurPipelines::run`] itself — reached from `WgpuCacheStore::frosted`
//! during the draw pass, so the passes land in the queue before the frame
//! that samples them and the ordering is the queue's. The chain only runs
//! when the source's epoch changed, so in a settled frame nothing here
//! executes at all.
//!
//! # Colour space
//!
//! Every target of the chain carries the recording format, and that format
//! decides what space the weighted sums happen in — which is not the same
//! in every build:
//!
//! - With `iced_graphics::color::GAMMA_CORRECTION` on, the format is
//!   sRGB-typed (`Rgba8UnormSrgb`). The hardware linearises on every read
//!   and re-encodes on every write, so the passes average in linear light.
//! - With the crate's default features (`web-colors`) it is `Rgba8Unorm`,
//!   and the passes average the stored bytes exactly as `blur::cpu`
//!   does.
//!
//! So the two backends agree in the default configuration and diverge only
//! in a gamma-corrected build. The divergence is not small: measured across
//! a black-to-white step at a residual sigma of 3 target texels, the peak
//! difference between an encoded-space and a linear-space blur is 73/255 —
//! 29 % of full range — and the second moment of the output profile is 3.55
//! against 3.00, an 18 % wider radius. The shared sigma is therefore a lie
//! on high-contrast edges in a gamma-corrected build, and collapses to
//! nothing for low-contrast content.
//!
//! The caveat that matters most is subtler. The hardware decodes the stored
//! **premultiplied** channels, so for alpha below one the GPU computes
//! `srgb_to_linear(straight * alpha)` rather than `linear(straight) *
//! alpha`. Over a translucent backdrop it is therefore not blurring in
//! linear light at all, but in a third space that neither backend's
//! calibration describes.
//!
//! Nothing here converts anything, which is deliberate: un-premultiplying
//! is what produces the dirty haloes `blur::cpu`'s tests exist to catch.
//! If parity is ever required, the resolution is to read through non-sRGB
//! views — `view_formats` on the cache texture in `record.rs::create_view`
//! and on every pool target — rather than to add a conversion to either
//! chain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use iced_core::Size;

use super::Blur;
use super::gaussian;

const SHADER: &str = include_str!("../shader/blur.wgsl");

/// Taps the WGSL `Params.taps` array holds — defined from
/// [`gaussian::MAX_TAPS`] rather than restated, so the shader and the
/// planner cannot drift apart.
const SHADER_MAX_TAPS: usize = gaussian::MAX_TAPS;

// The WGSL has no way to read a Rust constant, so the one place the two can
// be held together is here. Getting it wrong is a pipeline-creation failure
// that appears only on a machine with an adapter — `min_binding_size` stops
// matching the shader's block — and the naga test below parses the shader
// without comparing sizes, so it cannot catch it either.
const _: () = assert!(
    SHADER_MAX_TAPS == 8,
    "blur.wgsl hard-codes array<vec4<f32>, 8>; keep the two in step"
);

/// The uniform block one pass reads: where its source texels are, which way
/// to walk over them, and the tap table to walk with.
///
/// Laid out to match the WGSL `Params` struct byte for byte; `bytemuck`
/// only guarantees the bytes, so the padding is explicit on both sides.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    texel: [f32; 2],
    direction: [f32; 2],
    tap_count: u32,
    _pad: [u32; 3],
    /// `[offset, weight, 0, 0]` per tap.
    taps: [[f32; 4]; SHADER_MAX_TAPS],
}

const PARAMS_SIZE: u64 = std::mem::size_of::<Params>() as u64;

impl Params {
    /// The block for a pass that reads a level of `source` texels and walks
    /// along `direction` with `taps`.
    ///
    /// `texel` is the *source* level's, not the target's: the offsets the
    /// planner produced are in texels of the image being sampled, and a
    /// halving pass reads a level twice the size of the one it writes.
    fn new(source: Size<u32>, direction: [f32; 2], taps: &[(f32, f32)]) -> Self {
        let used = taps.len().min(SHADER_MAX_TAPS);
        let mut table = [[0.0_f32; 4]; SHADER_MAX_TAPS];

        for (slot, &(offset, weight)) in table.iter_mut().zip(&taps[..used]) {
            *slot = [offset, weight, 0.0, 0.0];
        }

        Self {
            texel: [
                1.0 / source.width.max(1) as f32,
                1.0 / source.height.max(1) as f32,
            ],
            direction,
            tap_count: used as u32,
            _pad: [0; 3],
            taps: table,
        }
    }
}

/// One rented render target.
pub(crate) struct Target {
    pub view: Arc<wgpu::TextureView>,
    size: Size<u32>,
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The view is a wgpu handle with nothing readable in it; the size is
        // the only thing a pool bug would show up in.
        f.debug_struct("Target")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// The targets of one size that nobody is renting, each with the number of
/// frames it has gone unrented.
type Idle = Vec<(Arc<wgpu::TextureView>, u32)>;

/// Intermediate render targets, rented by size and all returned before the
/// blur that rented them returns.
///
/// A returned target is kept for
/// [`DERIVED_IDLE_FRAMES`](super::DERIVED_IDLE_FRAMES) in case the next
/// blur wants the same size, and released by [`TargetPool::age`] after
/// that. The blur runs on invalidation only, so in a settled application
/// the pool empties itself within a second of the last one.
#[derive(Debug, Default)]
pub(crate) struct TargetPool {
    free: Mutex<HashMap<(u32, u32), Idle>>,
}

impl TargetPool {
    /// An empty pool. Targets are created on the first rent of each size.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A target of exactly `size`, reused if one is idle and created
    /// otherwise.
    ///
    /// The size is part of the key rather than a "big enough" match: a pass
    /// writes the whole attachment and reads normalised coordinates, so a
    /// larger target would blur into the wrong texels rather than merely
    /// waste memory.
    pub(crate) fn rent(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: Size<u32>,
    ) -> Target {
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(view) = free
            .get_mut(&(size.width, size.height))
            .and_then(Vec::pop)
            .map(|(view, _)| view)
        {
            return Target { view, size };
        }

        drop(free);
        Target {
            view: Arc::new(create_target(device, format, size)),
            size,
        }
    }

    /// Returns a target the chain has finished reading. Its idle count
    /// restarts, so it survives the next [`TargetPool::age`].
    pub(crate) fn give_back(&self, target: Target) {
        self.free
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry((target.size.width, target.size.height))
            .or_default()
            .push((target.view, 0));
    }

    /// Releases targets nobody has rented for
    /// [`DERIVED_IDLE_FRAMES`](super::DERIVED_IDLE_FRAMES). Called from
    /// `begin_frame`.
    pub(crate) fn age(&self) {
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        for slots in free.values_mut() {
            slots.retain_mut(|(_, idle)| {
                *idle += 1;
                *idle <= super::DERIVED_IDLE_FRAMES
            });
        }
        free.retain(|_, slots| !slots.is_empty());
    }
}

fn create_target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: Size<u32>,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("iced_texture_cache blur target"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Every intermediate keeps the recording format, and no view here
        // reinterprets it. That is what decides the space the passes sum
        // in; see this module's "Colour space" section, which is where the
        // consequences — and the divergence from the software chain in a
        // gamma-corrected build — are written down.
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// One encoded pass: which pipeline runs it, what it reads, what it writes
/// and the uniform block it reads them with.
///
/// A struct rather than four more arguments, because the two loops in
/// [`BlurPipelines::run`] differ in exactly these four things and in
/// nothing else.
struct Pass<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    source: &'a wgpu::TextureView,
    target: &'a wgpu::TextureView,
    params: Params,
}

/// The two pipelines of the chain and the bindings they share.
pub(crate) struct BlurPipelines {
    halve: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The recording format both pipelines were built against, and the one
    /// every target of the chain is created with.
    ///
    /// Kept here rather than passed to [`BlurPipelines::run`]: a pipeline
    /// is permanently bound to the format of its colour target, so a
    /// parameter would make a call these pipelines cannot serve
    /// expressible.
    format: wgpu::TextureFormat,
}

impl std::fmt::Debug for BlurPipelines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlurPipelines").finish_non_exhaustive()
    }
}

impl BlurPipelines {
    /// Compiles the shader and builds both pipelines against `format`.
    ///
    /// `format` is the recording format, which every target of the chain
    /// carries and which [`BlurPipelines::run`] reads back from `self`: one
    /// set of pipelines serves the whole chain only because no pass
    /// converts anything.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_texture_cache blur shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_texture_cache blur bind group layout"),
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
                        // One block per pass, each in a buffer of its own:
                        // a pass is encoded once and never revisited, so
                        // there is nothing for a dynamic offset to index.
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARAMS_SIZE),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iced_texture_cache blur pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = |label: &str, entry_point: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // No blending: a pass overwrites its target whole,
                        // and blending premultiplied weights into whatever
                        // the pooled target held last would be a ghost of
                        // an older blur.
                        blend: None,
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
            })
        };

        let halve = pipeline("iced_texture_cache blur halve pipeline", "fs_halve");
        let blur = pipeline("iced_texture_cache blur pipeline", "fs_blur");

        // Bilinear filtering is what makes both halves of the chain work:
        // the halving pass gets its box average from one fetch, and the
        // linear-sampled taps rely on the hardware putting two folded
        // weights back. Clamp-to-edge is the backdrop-filter edge rule.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iced_texture_cache blur sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });

        Self {
            halve,
            blur,
            layout,
            sampler,
            format,
        }
    }

    /// Runs the whole chain and returns the derived view and its size.
    ///
    /// `source` is the recorded texture; the result is `source_size`
    /// divided by `blur.downscale`. Every intermediate comes from `pool`
    /// and goes back into it before this returns.
    ///
    /// The `Arc` is taken by reference rather than by view: the degenerate
    /// chain hands the caller's own `Arc` straight back, and
    /// `composite.rs::texture_key` keys its bindings by `Arc::as_ptr`, so
    /// minting a second `Arc` for a texture the store already holds would
    /// give one texture two identities and a duplicate binding.
    pub(crate) fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &Arc<wgpu::TextureView>,
        source_size: Size<u32>,
        blur: Blur,
        pool: &TargetPool,
    ) -> (Arc<wgpu::TextureView>, Size<u32>) {
        // `Blur::resolve` only ever produces a power of two in `1..=8`. A
        // hand-built `Blur` that broke that would halve `trailing_zeros()`
        // times — thirty-two rent-and-encode iterations for `0`, followed
        // by a divide by zero inside `target_size` — so it is worth one
        // assertion rather than the `debug_assert_eq!` below noticing late.
        debug_assert!(
            blur.downscale.is_power_of_two(),
            "the chain halves, so the downscale is a power of two"
        );

        let chosen = gaussian::plan(blur.residual_sigma());
        // `downscale` is a power of two, so this is its base-two logarithm:
        // the number of halvings between the source and the target level.
        let halvings = blur.downscale.trailing_zeros();

        if halvings == 0 && chosen.passes == 0 {
            // Nothing to do, and reachable: a sigma of half a device pixel
            // resolves to `downscale == 1` with no residual pass. The
            // caller's own `Arc` goes back — not a pooled target, which the
            // pool would recycle under the derivative still holding it, and
            // not a fresh `Arc`, which would split the texture's identity.
            return (source.clone(), source_size);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("iced_texture_cache blur"),
        });

        // One uniform buffer per pass, parked here until the submit below:
        // a pass is encoded and its bind group dropped straight away, and
        // this is what guarantees the buffer it read outlives the encoder
        // rather than the details of wgpu's own reference counting.
        let mut uniforms = Vec::new();
        // The level the previous pass wrote. `None` while the chain is still
        // reading the caller's source, which is never returned to the pool.
        let mut written: Option<Target> = None;
        // The level the *next* pass reads.
        let mut level = source_size;

        for _ in 0..halvings {
            let next = Size::new(
                level.width.div_ceil(2).max(1),
                level.height.div_ceil(2).max(1),
            );
            let target = pool.rent(device, self.format, next);

            self.encode(
                device,
                queue,
                &mut encoder,
                &mut uniforms,
                &Pass {
                    pipeline: &self.halve,
                    source: written
                        .as_ref()
                        .map_or(source.as_ref(), |held| held.view.as_ref()),
                    target: &target.view,
                    // A halving pass takes no taps; the fragment entry point
                    // reads none. The texel size is still the source's,
                    // because that is what the sampler walks.
                    params: Params::new(level, [0.0, 0.0], &[]),
                },
            );

            if let Some(previous) = written.replace(target) {
                pool.give_back(previous);
            }
            level = next;
        }

        // Halving with the same `div_ceil` the software path rounds with is
        // what keeps the two backends' derivatives the same size; if they
        // ever disagreed, one residual sigma would stop describing both.
        debug_assert_eq!(
            level,
            blur.target_size(source_size),
            "the halving chain lands on the target level"
        );

        for _ in 0..chosen.passes {
            // Horizontal first, then vertical, both at the target level: a
            // separable Gaussian is the product of the two, in either order.
            for direction in [[1.0, 0.0], [0.0, 1.0]] {
                let target = pool.rent(device, self.format, level);

                self.encode(
                    device,
                    queue,
                    &mut encoder,
                    &mut uniforms,
                    &Pass {
                        pipeline: &self.blur,
                        source: written
                            .as_ref()
                            .map_or(source.as_ref(), |held| held.view.as_ref()),
                        target: &target.view,
                        params: Params::new(level, direction, &chosen.taps),
                    },
                );

                if let Some(previous) = written.replace(target) {
                    pool.give_back(previous);
                }
            }
        }

        queue.submit(Some(encoder.finish()));
        drop(uniforms);

        match written {
            // The last target written is the derivative and stays out of the
            // pool: the caller holds it until its source is re-recorded.
            Some(target) => (target.view, level),
            // Unreachable — the degenerate chain returned above — but a
            // draw pass is no place to panic, and the source is the honest
            // answer to "a chain that did nothing".
            None => (source.clone(), source_size),
        }
    }

    /// Encodes one pass into `encoder` and parks its uniform buffer in
    /// `uniforms`.
    fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut Vec<wgpu::Buffer>,
        pass: &Pass<'_>,
    ) {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iced_texture_cache blur params"),
            size: PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Queue writes are ordered against the submit below, so this lands
        // before the pass that reads it however late the submit happens.
        queue.write_buffer(&params, 0, bytemuck::bytes_of(&pass.params));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced_texture_cache blur bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(pass.source),
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

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_texture_cache blur pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: pass.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // A pooled target arrives with an older blur in it
                        // and the triangle covers every texel anyway, so the
                        // clear costs nothing and removes the question.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(pass.pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        uniforms.push(params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        let _ = validator.validate(&module).expect("WGSL validates");
    }

    #[test]
    fn the_uniform_block_holds_every_tap_a_plan_can_ask_for() {
        // The shader reads `tap_count` offsets out of a fixed-size array;
        // `gaussian::plan` adds passes rather than taps precisely so this
        // bound holds. Assert the two sides agree.
        assert_eq!(SHADER_MAX_TAPS, crate::blur::gaussian::MAX_TAPS);
        assert_eq!(PARAMS_SIZE % 16, 0, "a uniform block is 16-byte aligned");
    }
}
