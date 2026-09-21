# `iced_texture_cache`

Texture caching for [iced](https://iced.rs) 0.14 without an iced fork. Wrap an
expensive subtree in
`Cached`: it is rasterized once into a texture and composited as a single
textured quad on every following frame, optionally translated, scaled or
faded by an animated value from `iced_animate`. That is the *compositor tier*
of the animation engine. Moving the image does not run layout or record the
subtree again.

## Minimal example

```rust,no_run
use iced::widget::{column, text};
use iced::Vector;
use iced_texture_cache::iced_animate::{curves::SMOOTH, key, Motion};
use iced_texture_cache::{cached, Element, TextureCache};

struct App {
    motion: Motion,
    // The handle *is* the texture's identity: keep it in state.
    cache: TextureCache,
    open: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Toggle,
}

impl App {
    fn new() -> Self {
        Self {
            motion: Motion::new(),
            cache: TextureCache::new(),
            open: false,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle => self.open = !self.open,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let offset = self.motion.to(
            key!(),
            SMOOTH,
            if self.open { Vector::new(240.0, 0.0) } else { Vector::ZERO },
        );

        self.motion
            .host(column![cached(self.cache.clone(), text("expensive")).translate(offset)])
            .into()
    }
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view).run()
}
```

Add to `Cargo.toml`:

```toml
[dependencies]
iced = "0.14"
iced_texture_cache = "0.1"
```

`Element` is `iced::Element<'_, Message, Theme, iced_texture_cache::Renderer>`;
every stock iced widget is generic over the renderer, so nothing else in an
application changes. `iced_animate` is re-exported as
`iced_texture_cache::iced_animate` so the versions always match.

## Concepts

### Recording and compositing

`Cached` records its content when the cache is new or invalidated, or when the
content's size, the window's scale factor or the supersample factor changes.
Every other frame it composites the texture under the effective transform.
`translate`, `scale` and `opacity` take `Anim` values from the engine and bind
them at `Tier::Composite`; `supersample(f)` records at `f` times the device
resolution for content that will be enlarged.

### Invalidation

Invalidation is automatic when the content reacts to an event (captures it,
publishes a message, invalidates layout or widgets, requests a redraw, or
changes its hover appearance). Call `TextureCache::invalidate` for content
that changes without an event; `auto_invalidate(false)` makes that the only
event-driven trigger (size and scale changes and nested caches still
re-record). `TextureCache::generation()` counts invalidations. A nested
`Cached` is baked into its outer's texture, and an inner re-record forces the
outer to re-record in the same frame.

### Crispness

`pixel_snap(PixelSnap::Auto | Always | Never | LayoutOnly)` controls whether
the composited texel grid is snapped to the device-pixel grid. `Auto` (default)
snaps a pure translation once it is at rest, so the resting frame is
pixel-exact while motion stays smooth; it never snaps while a live scale is
bound. `LayoutOnly` snaps only the layout origin. `supersample_in_motion(true)`
records at ≥ 1.5× while moving and drops back at rest (one extra record per
rest↔motion transition).

### Reconstruction filter

A texture composited between device pixels has to be resampled, and
`FilterQuality` picks the kernel that does it:

| Tier | Cost | Look |
|---|---|---|
| `CatmullRom` | up to 9 hardware-bilinear taps, 3 for an axis-aligned slide, 1 at integer phase | sharpest; keeps moving text and edges crisp |
| `Bilinear` | one tap | glides smoothly, slightly soft while moving |
| `Snap` | one tap on the pixel grid | always crisp, motion steps by whole device pixels |

`Snap` **overrides `pixel_snap`**: its crispness comes from the geometry, not
from the shader, so it snaps whatever that policy asks for.

Without a choice the tier follows the graphics adapter — `CatmullRom` on a
discrete GPU, `Bilinear` on an integrated one, `Snap` on anything else
(software, virtual, unknown), where a fragment-heavy kernel would hurt most.
The software backend has no adapter and composites `Bilinear`; it has no
bicubic kernel either, so `CatmullRom` degrades to the same bilinear tap there.

`set_filter_quality(quality)` forces one tier for the whole process (call it
before `run()`; `set_filter_quality(None)` restores the automatic choice), and
`Cached::filter_quality` / `Pager::filter_quality` override it per widget. The
tier only affects compositing, so changing it never re-records a texture.

Two caveats, both sharpened versions of the mipmap limitation below: with
`supersample` above 1 the texel grid is finer than the device grid, so the
integer-phase fast path never fires and the full kernel runs on a minification
it cannot fix; and at a `scale` well below 1 `CatmullRom`'s high-frequency
boost makes the aliasing slightly more visible than `Bilinear` does.

### Warping

`Cached::genie` applies a non-affine collapse that `scale` cannot express.
`Warp::Genie` draws the content into one of its own corners the way a window
minimises into a dock icon. Its shape is taken from `KWin`'s magic lamp effects
and a published recreation of the macOS original: the content travels along
its axis toward the anchor, and a cubic shape curve argued by each row's
position *along that travel path* draws the rows into a neck one after
another, so the near edge necks to a point while the far edge still holds its
width. The stretch and the travel overlap, which is what stops it reading as
two animations. What passes beyond the anchor is consumed by it.

It is evaluated per destination pixel in the fragment shader as an inverse
map, so it costs no mesh, no extra draw call and no re-record — it binds at
`Tier::Composite` like `translate`, `scale` and `opacity`.

The map only ever shrinks the image inside its own rectangle, so nothing has
to make room for it and no sibling moves. That holds because progress is
clamped to `0..=1`; drive it with a curve that does not overshoot, such as
`curves::QUICK` or `curves::SMOOTH`. A bouncy curve is not wrong so much as
wasted — it flattens against the clamp.

While a warp is live the composite drops to `FilterQuality::Bilinear`: the
neck is severe minification, a cache texture has no mip chain, and
`CatmullRom`'s high-frequency boost makes that worse. The configured tier
returns at rest.

The software backend has no shaders, so the genie degrades there to a scale
about the same anchor at the same progress: the motion and its timing
survive, the neck does not.

### Z-order

Anything drawn *after* a `Cached` inside the same parent layer renders
**beneath** the cached texture (iced's layer stack reopens the previous layer
after the texture's clip; the same happens for `image`/`text` vs quads). Put
overlapping siblings in a `stack`, which gives each child its own layer. The
`cache_benchmark` example shows both.

### Pager

`Pager::new(pages).current(i)` is a horizontal stack of pages that slides
between them. While sliding, each visible page is recorded into its own
texture and composited under the slide, and the height interpolates between
the pages. It uses `Tier::Layout` and the `STRUCTURAL` curve by default;
`.curve(..)` changes the curve. At rest, it draws the current page directly
and snaps it. `.motion(m)`
binds it to an engine; without one it switches instantly.

`pixel_snap` and `filter_quality` apply to the *sliding* frames only — the
resting page is drawn directly on the device grid under every policy.

The two axes of a slide are not alike. `x` carries the motion and must stay
fractional or the page steps by whole device pixels; `y` moves only because
the pager interpolates its height between the two pages and centres each one
in the result. Leaving `y` fractional is expensive: with both axes off the
grid the composite runs its full 9-tap kernel and resamples the page
*vertically*, which is the direction text can least afford to lose. `Auto`
(the default) therefore snaps `y` and leaves `x` alone — smooth horizontally,
crisp vertically. `LayoutOnly` additionally snaps the pager's own `x` origin,
keeping only the slide fractional, so the blur level cannot breathe when the
surrounding layout shifts mid-slide. `Always` snaps both axes; `Never` snaps
neither and is an escape hatch, not a good default.

### Renderer and compositor

`Renderer` and `Compositor` are iced's own wgpu / tiny-skia fallback types
over thin wrappers that add cache storage and a per-window scale factor; iced's
`application(..)` picks them up from the `Element` alias. Backend selection
follows iced (`ICED_BACKEND=wgpu|tiny-skia` forces one);
`TextureRenderer::backend` reports which one is active. `TextureRenderer` is
the open trait a custom renderer implements to be cacheable: `record` returns
`Record::{Fresh, Reused, Uncacheable}`, and on `Uncacheable` the widget draws
its content in place.

### Surface format

With iced's `web-colors` on, the compositor never picks a float swapchain
format: it prefers `Bgra8Unorm`/`Rgba8Unorm`, then any integer non-sRGB
format. Stock iced can land in `Rgba16Float` on NVIDIA + Wayland and encode
sRGB twice ("washed-out" greys). `RUST_LOG=info` prints the adapter's format
list and the choice once the application installs a `log` backend (the
examples use `env_logger`).

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `wgpu` | yes | The GPU backend (`iced_wgpu`); textures are GPU textures. |
| `tiny-skia` | yes | The software backend; caches are pixmaps composited through the image pipeline, so it enables `iced_tiny_skia/image` (the `image` decoder) even without `image`. |
| `crisp` | yes | iced's text crispness (`iced_core/crisp`). |
| `web-colors` | yes | iced's sRGB-in-non-sRGB colour handling; off = gamma-correct linear colour (disable it in your `iced` line too). |
| `x11`, `wayland` | yes | Linux window platforms for the software path; no effect elsewhere. |
| `image` | no | The `image` widget. |
| `svg` | no | The `svg` widget. |
| `canvas` | no | Geometry (`canvas`). |
| `strict-assertions` | no | wgpu validation and debugging flags. |

At least one of `wgpu`, `tiny-skia` must be on (`compile_error!` otherwise).
Disabling a backend really removes it from the build. `thread-pool` and
`linux-theme-detection` belong to your own `iced` dependency line.

To turn `web-colors` off (gamma-correct linear colour), disable the defaults
on **both** lines and re-list what you need; the two lines must agree, since
`iced` and this crate share one `iced_renderer`:

```toml
[dependencies]
iced = { version = "0.14", default-features = false, features = ["wgpu", "tiny-skia", "thread-pool", "x11", "wayland"] }
iced_texture_cache = { version = "0.1", default-features = false, features = ["wgpu", "tiny-skia", "x11", "wayland"] }
```

## Limitations

* Paint-tier engine tracks *inside* a cached subtree are not detected (the
  engine ticks outside it); put an animating `Cached` outermost, or its outer
  will re-record every frame.
* The texture covers the whole layout box, not the visible part: a `Cached`
  around a long list inside a `scrollable` records the entire list.
* Only the cursor is mapped through `translate`/`scale`; positions carried by
  events reach the content untransformed, and overlays of *scaled* content
  open at the layout origin.
* Order follows draw order and layers; there is no explicit z-index.
* The scale factor used for recording lags one frame behind a DPI change.
* Textures are bounded by the device limit (GPU) or 16 384 px and 256 MiB
  (software); oversize content draws inline with a warning, without group
  opacity.
* A record borrows a nested renderer from a pool that holds one per nesting
  depth (a `Cached` inside a `Cached` takes a second one), so nothing is kept
  per cache except its texture. Composites are sampled bilinearly without
  mipmaps:
  keep `supersample ≤ 2 × scale`.
* Native only; wasm is not supported.

## Related crates

* [`iced_animate`](https://crates.io/crates/iced_animate): the engine
  (re-exported here).
* [`iced_page_router`](https://crates.io/crates/iced_page_router): pages and
  history.
* [`iced_luminate`](https://crates.io/crates/iced_luminate): a design kit built on
  all three.

Examples (`cargo run -p iced_texture_cache --example <name>` from a checkout;
they live in the
[repository](https://github.com/sonata-ltd/luminate/tree/master/texture_cache/examples)
and are not part of the published crate):

| Example | Shows | Environment |
|---|---|---|
| `compositor` | `translate`/`scale`/`opacity`, auto-invalidate, nesting | `ANIM_AUTOPLAY=1` flips every 1.5 s |
| `pager` | `Pager` | `ANIM_AUTOPLAY=1` advances every 1.5 s |
| `cache_benchmark` | a heavy scene cached vs direct, the z-order rule, all knobs in-UI | `BENCH_LOG=1` prints statistics to stderr |

Measurements are in
[BENCHMARKS.md](https://github.com/sonata-ltd/luminate/blob/master/texture_cache/BENCHMARKS.md);
changes in the workspace
[CHANGELOG](https://github.com/sonata-ltd/luminate/blob/master/CHANGELOG.md).
