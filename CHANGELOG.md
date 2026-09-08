# Changelog

All notable changes to the four crates of this workspace are documented here.
The crates are released together with the same version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). Before 1.0, minor versions may
break the API; every break is listed under **Changed** or **Removed**.

## [Unreleased]

The first release. What each crate provides:

### Changed

- Licensed the workspace under LGPL 2.1 or later. The bundled Inter font
  remains under the SIL Open Font License 1.1.

### Added

#### iced_animate

- `Motion`: a tree-external animation engine with keyed tracks (`MotionKey`,
  `key!`), `to`, `to_set`, `play`, `enter`, `retire`, `presence`, `get`,
  `end_build`, `collect`, `track_count`.
- `Anim<T>` handles resolved inside widgets; `Animatable` for `f32`, `Pixels`,
  `Vector`, `Point`, `Size`, `Rectangle`, `Radians`, `Color`, `Padding`,
  `Radius` (`MAX_COMPONENTS` = 4); `AnimLength`; `motion_set!` / `MotionSet`.
- Curves: closed-form springs (`SpringParams::new(bounce, duration)`) and
  eases (`Easing`), `Curve::delayed`; presets `curves::{SMOOTH, QUICK, BOUNCY,
  STRUCTURAL, FADE, COLLAPSE}`.
- A spring's `duration` is a perceptual duration, calibrated exactly as
  SwiftUI's `Spring(duration:bounce:)` is (`stiffness = (2π / duration)²`,
  `damping = (1 - bounce) · 4π / duration`), so values quoted for Apple's
  springs can be used unchanged. The spring is ~99 % of the way there when the
  duration elapses; full settling takes about 1.6x longer. The spring stops
  the frame nothing visible remains ahead of it — the excursion still to
  come, not the speed right now — so it never lingers below a pixel long
  enough to drift past its target and turn around. The shipped presets
  run shorter than Apple's own, whose durations are tuned for touch.
- `curves::sharp::{SMOOTH, QUICK, BOUNCY, STRUCTURAL}`: the same four springs
  about 1.6× brisker again, for interfaces that want to feel immediate.
- `Tier::{Composite, Paint, Layout}` invalidation per track; `TickStatus`.
- A frame spends real elapsed time, but only time a frame could have been
  drawn in. Frames are produced on demand, so an interface at rest draws none
  at all, and the gap before an animation starts is not motion anybody missed:
  the frame that follows a tick which left everything settled restarts the
  clock and advances nothing, exactly as an engine's first tick does. Past
  that, one frame spends at most 1/15 s, so a stall — a pipeline compiling, a
  window returning from behind another — resumes an animation where it stopped
  instead of teleporting it to the end.
- Widgets `widget::{Shape, Sized, Host}` with `shape()`, `sized()`, `host()`
  (under `widget` only; the engine and value types live at the root).
- `Shape::pixel_snap(PixelSnap::{Auto, Always, Never})`: when the quad is
  rounded to the pixel grid. iced's `crisp` feature rounds *both* edges of a
  quad, so a moving one changes size in whole-pixel lurches — but switching
  the rounding on or off displaces the shape by up to a pixel, which makes
  *when* it changes as visible as whether it is on.
  The default `Auto` snaps only while the bounds stand still, and a shape
  whose size is bound to a track never snaps at all, settled or not: the
  alternative is a step onto the grid on the one frame the motion ends. A
  shape moved by an *ancestor* still pays that step — it can see its own
  tracks, but only that its bounds changed, never by how much of a device
  pixel — so `Never` is the answer there.
- Depends on `iced_core` and `log` only.

#### iced_texture_cache

- `Cached` / `cached()`: records a subtree into a texture and composites it
  with animated `translate`, `scale`, `opacity`; `supersample`,
  `supersample_in_motion`, `pixel_snap(PixelSnap::{LayoutOnly, Auto, Always,
  Never})`, `filter_quality`, `auto_invalidate`; automatic invalidation
  when the content reacts, nested caches propagate to their ancestors.
  `LayoutOnly` is the default: it is the only policy whose rendering never
  switches modes, so a layer carried by an animating ancestor cannot glide
  sub-pixel and then land on the grid in a single frame at the end of the
  transition. `Auto` keeps the smoother motion and pays for it with that
  frame.
- `FilterQuality::{CatmullRom, Bilinear, Snap}`: the reconstruction kernel used
  when a texture is composited between device pixels. Chosen from the graphics
  adapter by default (`CatmullRom` discrete, `Bilinear` integrated, `Snap`
  otherwise; `Bilinear` on the software backend), forced process-wide with
  `set_filter_quality` and read back with `filter_quality`, and overridden per
  widget by `Cached::filter_quality` / `Pager::filter_quality`. `Snap`
  overrides `PixelSnap`.
- `Pager` / `pager()`: a sliding page stack with per-page textures and
  interpolated height (`current`, `motion`, `curve`, `width`, `max_height`,
  `filter_quality`, `pixel_snap`). `pixel_snap` applies to the sliding frames
  only and treats the axes separately: both `LayoutOnly` (the default) and
  `Auto` snap the vertical axis, which a slide moves only through its height
  interpolation, and leave the horizontal one fractional so the slide still
  glides. That puts one axis at integer phase, where the composite shader
  collapses its kernel from 9 taps to 3 and stops resampling the page
  vertically. `LayoutOnly` additionally snaps the pager's own horizontal
  origin, so a layout shift under a running slide cannot move the resampling
  phase; only the slide does.
- `ContentStack` / `content_stack()`: the same sliding page stack reached
  from the other side. A `Pager` puts the slide in the layout, so
  hit-testing and overlays need no correction and the composited origin has
  to be split into the moving part and the still one; a `ContentStack` lays
  its pages out in a fixed row and puts the slide in the *composite*. Because
  a page's layout box does not move, there is nothing for the device grid to
  quantise, and the resting frame is computed by the same expression as the
  sliding one — so the frame the stack stops compositing and starts drawing
  its page directly puts every pixel where the previous frame had it. The
  price is that everything asking "where is this page?" is corrected by hand.
  Reach for it when the transition has to be flawless; `Pager` when that
  correction is not acceptable.
- `TextureCache` handles with `id`, `invalidate`, `is_invalidated`,
  `record_count`, `generation`; `TextureCacheId`.
- `Renderer` and `Compositor` for wgpu and tiny-skia (optional features, at
  least one required), the `Element` alias, `Backend`, and the open
  `TextureRenderer` trait (`record` → `Record::{Fresh, Reused, Uncacheable}`,
  `draw_cached`, `filter_quality`).
- Surface-format selection that never picks a float format for web colours.
- Re-exports `iced_animate`.

#### iced_page_router

- `Router<Context, Theme, Renderer>` with `add`, `navigate`, `navigate_with`,
  `navigate_index`, `replace`, `back`, `forward`, bounded deduplicating history
  (`history_len`), `mouse_navigation`, `pages()` → `PageInfo`, typed `page` /
  `page_mut` / `message`, `view`, `update`, `subscription`.
- `Page` trait with `Lifecycle::{Drop, Suspend, Resident}`, `on_enter`,
  `on_navigate(options)`, `on_suspend`, `on_resume`, `into_snapshot`,
  `restore`, `background_subscription`.
- `Action` (`none`, `task`, `navigate`, `navigate_with`, `back`, `forward`,
  `replace`, `and_task`, `and_navigate`), `Navigation`, `RouteMessage`,
  `PageMessage`, `Payload`, `NavigationError`.
- `Registry` keyed by `Key` types holding `Shared<T>` values.
- Depends on `iced_core`, `iced_runtime` and `log` only.

#### iced_luminate

- `Luminate`: theme + motion engine; `host`, `button`, `input`, `sidebar`,
  `card`, `pager`; `Luminate::fonts()`.
- `Luminate::fonts()` returns eight buffers, not two: the upright and italic
  faces, plus one copy of each per weight in
  `typography::DECLARED_WEIGHTS` (500, 600, 700), differing only in
  `OS/2.usWeightClass`. The text stack selects a face by *exact* declared
  weight and falls back to any other family that declares it, and a variable
  font declares one — so without the copies, asking the kit's family for
  Medium silently rendered in a system font. The copies keep their
  `fvar`/`gvar` tables, so each is still the variable face that renders an
  animated weight between the declared ones. Each costs about 900 KB.
- `descriptor::{Button, ButtonContent, ButtonHierarchy, ButtonSize, Input,
  Sidebar, Axis, Card, Pager}` as plain data with builders.
- `theme::Theme` (`LIGHT`, `DARK`) implementing `iced::theme::Base` and the
  `Catalog`s of every widget the kit uses; token structs,
  `palette::{Palette, ColorScale}`, `typography::{TextStyle, TextSize,
  DisplaySize, TypographyTheme, styled_text, FONT, FAMILY}`; bundled Inter
  (OFL-1.1) behind `bundled-font`.
- Widgets `widget::{multi_border::MultiBorder, sidebar::Sidebar,
  error_bubble::ErrorBubble}` with `multi_border()`, `sidebar()`,
  `error_bubble()`; every item has one public path.
- Re-exports `iced`, `iced_animate` (as `animate`), `iced_page_router` (as
  `router`) and `iced_texture_cache` (as `texture`); `Element`, `Renderer`,
  `Router` aliases.
