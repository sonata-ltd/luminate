# Radial gradients

The public API is `iced_luminate::texture::{Radial, radial_gradient}` (also
available directly from `iced_texture_cache`). A fill is paint, not content
ownership or cache policy. It is a leaf combined with ordinary `stack`,
`container` and `cached`; no special `Cached::gradient` method is needed.

- `texture_cache/src/radial/mod.rs`: descriptor, widget and `RadialRenderer`.
- `texture_cache/src/radial/gpu.rs`: one shared Iced `Pipeline`, per-draw uniform
  slots pooled across frames. Parameters belong to primitives, so several
  gradients prepared before any draw never overwrite each other. `trim` resets
  the slot cursor and releases unused buffers after page changes.
- `texture_cache/src/shader/radial.wgsl`: viewport triangle, elliptical distance,
  premultiplied stop interpolation, logical corner radii and physical pixel AA.
- `texture_cache/src/radial/software.rs`: one bounded raster per widget; no
  process-wide raster cache. Opacity is applied during image composition.
- `texture_cache/src/renderer.rs`: backend-specific dispatch, including nested
  renderers used by existing texture recording.
- `luminate/examples/overview/pages/radial.rs`: direct/cached/interactive samples.

Iced retains ownership of device, queue, format, scissor and presentation. The
primitive implements `draw` to reuse Iced's render pass. The vertex shader uses
the full primitive viewport, so clipping does not move the gradient centre.
Inherited renderer transformations place the quad normally. There are no
backend patches, asynchronous device creation, custom global clocks or timers.

The widget has no children and relies on Iced's default pass-through event,
mouse and operation behaviour. Animated values are paint-tier, sampled in draw;
update compares the resolved paint, propagates invalidation to cached ancestors
and requests redraws while tracks move. Resting paint requests no ongoing work.

Tests cover descriptor limits, degenerate fills, transparent interpolation,
software pixels/DPI, raster reuse, input through overlays, cached rebuilds,
WGSL validation/layout and (ignored unless explicitly run) rendered GPU pixels
for multiple instances, clipping, repeated frames and nested texture recording.
