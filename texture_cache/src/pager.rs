//! [`Pager`]: a horizontal row of pages that slides between them.

use iced_animate::curves::STRUCTURAL;
use iced_animate::{Anim, Curve, Motion, MotionKey, Tier};
use iced_core::layout::{Layout, Limits, Node};
use iced_core::widget::{Id, Operation, Tree, tree};
use iced_core::{
    Clipboard, Element, Event, Length, Pixels, Point, Rectangle, Shell, Size, Transformation,
    Vector, Widget, mouse, overlay, renderer, window,
};

use crate::ancestors;
use crate::cached::PixelSnap;
use crate::filter::FilterQuality;
use crate::geometry::{composite_geometry, lerp, pager_page_bounds, snap_to_grid};
use crate::reaction::{Activity, observe};
use crate::record::{Record, TextureRenderer};
use crate::texture_cache::TextureCache;

/// Pages are recorded edge to edge: they are clipped to the pager anyway.
const BLEED: u32 = 0;

/// A horizontal row of pages that slides between them.
///
/// Only the page(s) the "camera" can see are laid out and drawn. While the
/// slide runs, each visible page is recorded once into its own texture and
/// composited under the slide offset; at rest the current page is drawn
/// directly (snapped to the device grid), so the idle frame costs nothing
/// extra and stays sharp. The pager's height interpolates between the
/// outgoing and incoming page, which is why the slide is a layout-tier
/// animation.
///
/// Mid-slide, pages are centred vertically in the interpolated height (the
/// taller page is clipped equally top and bottom), and the slide position is
/// clamped to the first and last page, so a bouncy curve never overshoots
/// past the ends. A view rebuild during a slide re-records the visible pages,
/// so content that changes without an event (a subscription tick) is not
/// shown stale.
///
/// This is the crate's one *composed* widget: it binds an `iced_animate`
/// [`Motion`] and a [`Curve`] itself, where [`Cached`](crate::Cached) only
/// consumes [`Anim`] values.
///
/// # Pages and `current`
///
/// Pages are addressed by index, so every pushed page counts, including
/// zero-sized ones. [`current`](Self::current) is clamped to the last page
/// (a `current` of 7 on a three-page pager shows page 2); an empty pager
/// lays out and draws nothing. Pages removed by a rebuild clamp the current
/// and pending indices the same way.
///
/// # Width and height
///
/// * [`width`](Self::width): `Fill` (the default) and `FillPortion` take the
///   available width, or, inside a parent with unbounded width, the width of
///   the widest visible page; `Fixed` sets it exactly; `Shrink` is the width
///   of the widest visible page. A page whose own width is `Fill` contributes
///   nothing to that measurement.
/// * [`max_height`](Self::max_height) caps the pager's height; pages are laid
///   out within it and anything that overflows is clipped.
///
/// # Motion
///
/// Bind it to a [`Motion`] with [`motion`](Self::motion) and place it inside
/// a [`Host`](iced_animate::widget::Host); without an engine the pager
/// switches pages in one frame.
/// [`curve`](Self::curve) picks the slide curve (default
/// [`STRUCTURAL`]).
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_texture_cache::iced_animate::Motion;
/// use iced_texture_cache::pager;
///
/// struct App { motion: Motion, page: usize }
///
/// impl App {
///     fn view(&self) -> iced_texture_cache::Element<'_, ()> {
///         let pages = [text("first"), text("second"), text("third")];
///         self.motion
///             .host(pager(pages).current(self.page).motion(self.motion.clone()))
///             .into()
///     }
/// }
/// ```
pub struct Pager<'a, Message, Theme = iced_core::Theme, Renderer = crate::Renderer> {
    id: Option<Id>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    width: Length,
    max_height: f32,
    current: usize,
    /// Engine driving the slide. Without one the pager jumps straight to the
    /// requested page instead of animating against a clock nothing ticks.
    motion: Option<Motion>,
    curve: Curve,
    /// `None` inherits the renderer's tier; see [`Pager::filter_quality`].
    filter: Option<FilterQuality>,
    pixel_snap: PixelSnap,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Pager<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pager")
            .field("id", &self.id)
            .field("pages", &self.children.len())
            .field("width", &self.width)
            .field("max_height", &self.max_height)
            .field("current", &self.current)
            .field("motion", &self.motion)
            .field("curve", &self.curve)
            .field("filter", &self.filter)
            .field("pixel_snap", &self.pixel_snap)
            .finish_non_exhaustive()
    }
}

/// A [`Pager`] over `children`. See [`Pager`].
#[must_use]
pub fn pager<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
) -> Pager<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    Pager::new(children)
}

/// Per-page bookkeeping.
#[derive(Debug)]
struct Page {
    cache: TextureCache,
    activity: Activity,
    last_interaction: mouse::Interaction,
    /// The cache generation last propagated to the pager's ancestors.
    propagated: u64,
}

impl Page {
    fn new() -> Self {
        Self {
            cache: TextureCache::new(),
            activity: Activity::default(),
            last_interaction: mouse::Interaction::None,
            propagated: 0,
        }
    }
}

/// Where the pager is and where it is heading.
#[derive(Debug)]
struct Switch {
    current: usize,
    pending: Option<usize>,
}

#[derive(Debug)]
struct State {
    switch: Switch,
    /// Identity of this pager's slide track: it has the lifetime of this
    /// `tree::State`, not an application-level identity.
    key: MotionKey,
    /// Position in page units: `1.5` is half way between pages 1 and 2.
    slide: Anim<f32>,
    pages: Vec<Page>,
    /// Pages laid out this frame, ascending and without duplicates: the one
    /// or two under the camera, plus the page a pending switch is heading
    /// for so operations (focus) reach it. Layout children are emitted in
    /// this order, so index order == layout-child order.
    visible: [Option<usize>; 3],
}

/// The one or two pages under the camera at a clamped `position`.
#[derive(Debug, Clone, Copy)]
struct Camera {
    first: usize,
    second: usize,
    position: f32,
}

fn camera(position: f32, page_count: usize) -> Camera {
    let last = page_count.saturating_sub(1) as f32;
    let position = if position.is_nan() {
        0.0
    } else {
        position.clamp(0.0, last)
    };
    Camera {
        first: position.floor() as usize,
        second: position.ceil() as usize,
        position,
    }
}

/// The canonical visible set: camera pages plus the pending target, in
/// range, ascending, deduplicated.
fn visible_indices(
    camera: Camera,
    pending: Option<usize>,
    page_count: usize,
) -> [Option<usize>; 3] {
    let mut set = [Some(camera.first), Some(camera.second), pending];
    for slot in &mut set {
        if slot.is_some_and(|i| i >= page_count) {
            *slot = None;
        }
    }
    // `None` sorts first; push it to the back so `laid_out_indices` sees a prefix.
    set.sort_unstable_by_key(|slot| slot.unwrap_or(usize::MAX));
    for i in 1..set.len() {
        if set[i].is_some() && set[i] == set[i - 1] {
            set[i] = None;
        }
    }
    set.sort_unstable_by_key(|slot| slot.unwrap_or(usize::MAX));
    set
}

/// Iterates the laid-out page indices in layout-child order.
fn laid_out_indices(visible: &[Option<usize>; 3]) -> impl Iterator<Item = usize> + '_ {
    visible.iter().flatten().copied()
}

/// Pairs every laid-out page index with its layout.
fn visible_pages(
    visible: [Option<usize>; 3],
    layout: Layout<'_>,
) -> impl Iterator<Item = (usize, Layout<'_>)> {
    visible.into_iter().flatten().zip(layout.children())
}

impl State {
    fn new(motion: Option<&Motion>, curve: Curve, page_count: usize, current: usize) -> Self {
        let mut state = Self {
            switch: Switch {
                current: current.min(page_count.saturating_sub(1)),
                pending: None,
            },
            key: MotionKey::unique(),
            slide: Anim::constant(0.0),
            pages: (0..page_count).map(|_| Page::new()).collect(),
            visible: [None; 3],
        };

        state.retarget(motion, curve);
        state
    }

    /// Points the slide track at the page the pager is heading for.
    ///
    /// Idempotent, and called on every rebuild so the track is touched each
    /// build and never collected by the engine while the pager lives.
    fn retarget(&mut self, motion: Option<&Motion>, curve: Curve) {
        let target = self.switch.pending.unwrap_or(self.switch.current) as f32;

        let Some(motion) = motion else {
            self.slide = Anim::constant(target);
            return;
        };

        self.slide = motion.to(self.key, curve, target);

        // `layout` interpolates the pager's height from this value, so it
        // is a layout-tier animation.
        self.slide.mark_tier(Tier::Layout);
    }

    fn position(&self) -> f32 {
        self.slide.get()
    }

    fn is_sliding(&self) -> bool {
        self.slide.is_animating()
    }
}

impl<'a, Message, Theme, Renderer> Pager<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// A pager over `children`, showing page 0.
    #[must_use]
    pub fn new(
        children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
    ) -> Self {
        let iterator = children.into_iter();
        Self::with_capacity(iterator.size_hint().0).extend(iterator)
    }

    /// An empty pager with room for `capacity` pages.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id: None,
            children: Vec::with_capacity(capacity),
            width: Length::Fill,
            max_height: f32::INFINITY,
            current: 0,
            motion: None,
            curve: STRUCTURAL,
            filter: None,
            pixel_snap: PixelSnap::default(),
        }
    }

    /// Sets the [`PixelSnap`] policy for the sliding pages (default
    /// [`PixelSnap::Auto`]).
    ///
    /// Only the *sliding* frames are affected: a resting page is drawn
    /// directly on the device grid, with no texture and so nothing to
    /// resample, and that stays true under every policy.
    ///
    /// A slide is horizontal, so the two axes are not alike here. `x` carries
    /// the motion and has to stay fractional or the page steps by whole
    /// device pixels. `y` moves only because the pager interpolates its
    /// height between the two pages and centres each one in the result — a
    /// side effect of the transition, not motion anyone follows. Leaving `y`
    /// fractional costs a great deal: with both axes off the grid the
    /// composite shader runs its full 9-tap kernel and resamples the page
    /// *vertically*, which is the direction text can least afford to lose.
    /// Snapping `y` puts that axis at integer phase, collapses the kernel to
    /// 3 taps, and keeps the slide just as smooth.
    ///
    /// * [`Auto`](PixelSnap::Auto) snaps `y` and leaves `x` alone: smooth
    ///   horizontally, crisp vertically. The default, and what you want.
    /// * [`LayoutOnly`](PixelSnap::LayoutOnly) also snaps the *pager's* own
    ///   `x` origin, keeping only the slide itself fractional, so the
    ///   resampling phase cannot shift when the surrounding layout moves
    ///   mid-slide (a window resize, a scrollbar appearing) — which would
    ///   otherwise make the blur level visibly breathe.
    /// * [`Always`](PixelSnap::Always) snaps both axes: crisp throughout, but
    ///   the slide steps by whole device pixels.
    /// * [`Never`](PixelSnap::Never) snaps neither and resamples both axes.
    ///   The escape hatch, not a good default.
    ///
    /// [`FilterQuality::Snap`] overrides all of them.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iced::widget::text;
    /// use iced_texture_cache::{PixelSnap, pager};
    ///
    /// let _: iced_texture_cache::Element<'_, ()> = pager([text("one"), text("two")])
    ///     .pixel_snap(PixelSnap::LayoutOnly)
    ///     .into();
    /// ```
    #[must_use]
    pub fn pixel_snap(mut self, mode: PixelSnap) -> Self {
        self.pixel_snap = mode;
        self
    }

    /// Overrides the [`FilterQuality`] used to composite the sliding pages.
    ///
    /// Without it the pager inherits the renderer's tier. Only the sliding
    /// frames are composited, so this changes nothing at rest, where a page is
    /// drawn directly on the device grid. [`FilterQuality::Snap`] snaps each
    /// page's texture onto that grid while it slides.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iced::widget::text;
    /// use iced_texture_cache::{FilterQuality, pager};
    ///
    /// let _: iced_texture_cache::Element<'_, ()> = pager([text("one"), text("two")])
    ///     .filter_quality(FilterQuality::Bilinear)
    ///     .into();
    /// ```
    #[must_use]
    pub fn filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter = Some(quality);
        self
    }

    /// The page to show (default 0). Clamped to the last page; see
    /// "Pages and `current`" on [`Pager`].
    #[must_use]
    pub fn current(mut self, index: usize) -> Self {
        self.current = index;
        self
    }

    /// Binds this pager's slide animation to `motion`. Without it the pager
    /// still works, but switches pages in one frame. The pager must sit
    /// inside a [`Host`](iced_animate::widget::Host) for the engine to advance.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// The curve of the slide (default [`STRUCTURAL`]). Only used with
    /// [`motion`](Self::motion).
    #[must_use]
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Appends a page. Pages are addressed by index, so every push counts,
    /// including zero-sized ones.
    #[must_use]
    pub fn push(mut self, child: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Appends several pages.
    #[must_use]
    pub fn extend(
        self,
        children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    /// Sets the widget id used by operations.
    #[must_use]
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Caps the pager's height; pages are laid out within it and anything
    /// that overflows is clipped.
    #[must_use]
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = max_height.into().0;
        self
    }

    /// Sets the width. See "Width and height" on [`Pager`].
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pager<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(
            self.motion.as_ref(),
            self.curve,
            self.children.len(),
            self.current,
        ))
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);

        let state = tree.state.downcast_mut::<State>();

        state.pages.resize_with(self.children.len(), Page::new);

        // Pages may have been removed since the indices were recorded.
        let last = self.children.len().saturating_sub(1);
        state.switch.current = state.switch.current.min(last);
        state.switch.pending = state.switch.pending.map(|pending| pending.min(last));

        let current = self.current.min(last);
        let heading_to = state.switch.pending.unwrap_or(state.switch.current);

        if heading_to != current {
            state.switch.pending = Some(current);

            // The pages the slide will pass over are recorded fresh.
            let from = state.switch.current;
            for page in &state.pages[from.min(current)..=from.max(current)] {
                page.cache.invalidate();
            }
        }

        if state.is_sliding() {
            // A rebuild mid-slide (idle or retargeting) may have changed page
            // content without any event reaching the page: re-record what
            // is on screen.
            for i in laid_out_indices(&state.visible) {
                if let Some(page) = state.pages.get(i) {
                    page.cache.invalidate();
                }
            }
        }

        state.retarget(self.motion.as_ref(), self.curve);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Shrink,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let max_height = limits.max().height.min(self.max_height);
        let page_count = self.children.len();

        let (camera, pending) = {
            let state = tree.state.downcast_ref::<State>();
            (camera(state.position(), page_count), state.switch.pending)
        };
        let visible = visible_indices(camera, pending, page_count);

        // The width is either known up front or the widest visible page,
        // measured unbounded. Each page is laid out once: a measured node is
        // reused when it already has the final width.
        let known_width = match self.width {
            Length::Fixed(width) => Some(width),
            Length::Shrink => None,
            _ => limits.max().width.is_finite().then_some(limits.max().width),
        };

        let mut nodes: [Option<(usize, Node)>; 3] = [None, None, None];
        let width = if let Some(width) = known_width {
            width
        } else {
            {
                let measure = Limits::new(Size::ZERO, Size::new(f32::INFINITY, max_height));
                let mut widest: f32 = 0.0;
                for (slot, i) in nodes.iter_mut().zip(laid_out_indices(&visible)) {
                    let node = self.children[i].as_widget_mut().layout(
                        &mut tree.children[i],
                        renderer,
                        &measure,
                    );
                    let page_width = node.size().width;
                    if page_width.is_finite() {
                        widest = widest.max(page_width);
                    }
                    *slot = Some((i, node));
                }
                widest
            }
        };

        let child_limits = Limits::new(Size::new(width, 0.0), Size::new(width, max_height));
        for (slot, i) in nodes.iter_mut().zip(laid_out_indices(&visible)) {
            let reusable = slot
                .as_ref()
                .is_some_and(|(_, node)| node.size().width == width);
            if !reusable {
                *slot = Some((
                    i,
                    self.children[i].as_widget_mut().layout(
                        &mut tree.children[i],
                        renderer,
                        &child_limits,
                    ),
                ));
            }
        }

        let height_of = |index: usize| -> f32 {
            nodes
                .iter()
                .flatten()
                .find(|(i, _)| *i == index)
                .map_or(0.0, |(_, node)| node.size().height)
        };
        let height = lerp(
            height_of(camera.first),
            height_of(camera.second),
            camera.position.fract(),
        );

        // Pages are positioned by the slide itself, so every walk (operate,
        // events, overlays, draw) sees them where they really are.
        let offset_x = camera.position * width;
        let children = nodes
            .into_iter()
            .flatten()
            .map(|(i, node)| {
                let page_height = node.size().height;
                node.move_to(Point::new(
                    i as f32 * width - offset_x,
                    (height - page_height) / 2.0,
                ))
            })
            .collect();

        tree.state.downcast_mut::<State>().visible = visible;

        Node::with_children(Size { width, height }, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let visible = tree.state.downcast_ref::<State>().visible;

        operation.container(self.id.as_ref(), layout.bounds());
        operation.traverse(&mut |operation| {
            for (i, child_layout) in visible_pages(visible, layout) {
                self.children[i].as_widget_mut().operate(
                    &mut tree.children[i],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let redraw_now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            _ => None,
        };

        let Tree {
            state,
            children: trees,
            ..
        } = tree;
        let State {
            switch,
            slide,
            pages,
            visible,
            ..
        } = state.downcast_mut::<State>();

        // `iced_animate::widget::Host` has already advanced the clock for this
        // frame, so the arrival can simply be observed here. The pending page
        // must leave the layout (and `visible`) once it is current.
        if redraw_now.is_some()
            && !slide.is_animating()
            && let Some(next) = switch.pending.take()
        {
            switch.current = next;
            shell.invalidate_layout();
            shell.request_redraw();
            // The resting page is drawn directly from now on: an enclosing
            // texture must pick up the final pose.
            ancestors::invalidate_ancestors();
        }

        // At rest the current page is drawn directly, so its texture (and
        // the reaction bookkeeping that keeps it fresh) is irrelevant: the
        // next switch invalidates the whole range anyway.
        let sliding = slide.is_animating();

        // Mid-slide the page textures composite at a moving offset: the
        // pager's own image changes every frame even when no page cache
        // does, so an enclosing texture re-records each frame.
        if sliding && redraw_now.is_some() {
            ancestors::invalidate_ancestors();
        }

        for (i, child_layout) in visible_pages(*visible, layout) {
            let page = &mut pages[i];
            let child = &mut self.children[i];
            let child_tree = &mut trees[i];

            let mut local_messages = Vec::new();
            let mut local = Shell::new(&mut local_messages);

            // A `Cached` inside this page is baked into the page texture.
            ancestors::with_ancestor(&page.cache, || {
                child.as_widget_mut().update(
                    child_tree,
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    &mut local,
                    viewport,
                );
            });

            if sliding {
                // A hover appearance change the page applies silently on
                // `RedrawRequested` (the page moving under a static cursor).
                let interaction_changed = redraw_now.is_some() && {
                    let interaction = child.as_widget().mouse_interaction(
                        child_tree,
                        child_layout,
                        cursor,
                        viewport,
                        renderer,
                    );
                    let changed = page.last_interaction != interaction;
                    page.last_interaction = interaction;
                    changed
                };

                if observe(&local, redraw_now, interaction_changed, &mut page.activity) {
                    page.cache.invalidate();
                }
            }

            // A page texture is baked into every enclosing texture.
            let _ = ancestors::propagate(&page.cache, &mut page.propagated);

            shell.merge(local, |m| m);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let visible = tree.state.downcast_ref::<State>().visible;

        visible_pages(visible, layout)
            .map(|(i, child_layout)| {
                self.children[i].as_widget().mouse_interaction(
                    &tree.children[i],
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if self.children.is_empty() {
            return;
        }

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let scale = renderer.scale_factor();

        let filter = self.filter.unwrap_or_else(|| renderer.filter_quality());

        if state.is_sliding() {
            for (i, child_layout) in visible_pages(state.visible, layout) {
                let child_bounds = child_layout.bounds();
                // The page a switch is heading for is laid out but may be
                // entirely off-screen: nothing to record yet.
                if !child_bounds.intersects(&bounds) {
                    continue;
                }
                // The slide has no transform of its own: the offset lives in
                // the page's layout, so the policy is applied to the origin
                // the texture is composited at. `composite_geometry` is then
                // handed the result and must not snap again.
                let composite = composite_geometry(
                    BLEED,
                    pager_page_bounds(filter, self.pixel_snap, child_bounds, bounds, scale),
                    scale,
                    1.0,
                    None,
                );
                // Clip to the pager (a page mid-slide overhangs its edges)
                // and to the parent's clip.
                let Some(clip) = composite
                    .cache_bounds
                    .intersection(&bounds)
                    .and_then(|clip| clip.intersection(viewport))
                else {
                    continue;
                };

                let child = &self.children[i];
                let child_tree = &tree.children[i];
                let page = &state.pages[i];
                // The page's content lands `BLEED` texels into the texture
                // whatever origin the texture is composited at: the snap then
                // displaces the image, instead of baking a sub-pixel phase
                // into a texture that is recorded once and reused all slide.
                let origin =
                    Vector::new(BLEED as f32 - child_bounds.x, BLEED as f32 - child_bounds.y);

                let record = renderer.record(
                    &page.cache,
                    composite.physical,
                    composite.texture_scale,
                    |r| {
                        r.with_translation(origin, |r| {
                            child.as_widget().draw(
                                child_tree,
                                r,
                                theme,
                                style,
                                child_layout,
                                cursor,
                                &child_bounds,
                            );
                        });
                    },
                );

                match record {
                    Record::Fresh | Record::Reused => renderer.draw_cached(
                        &page.cache,
                        composite.cache_bounds,
                        clip,
                        Transformation::IDENTITY,
                        1.0,
                        filter,
                    ),
                    // Too large for a texture: the page is already laid out
                    // where it is drawn, so draw it there under the same clip.
                    Record::Uncacheable => renderer.with_layer(clip, |r| {
                        child.as_widget().draw(
                            child_tree,
                            r,
                            theme,
                            style,
                            child_layout,
                            cursor,
                            &clip,
                        );
                    }),
                }
            }
        } else {
            renderer.with_layer(bounds, |renderer| {
                for (i, child_layout) in visible_pages(state.visible, layout) {
                    let child_bounds = child_layout.bounds();
                    if !child_bounds.intersects(&bounds) {
                        continue;
                    }
                    // Snap the resting page to the device grid so text stays crisp.
                    let snap = Vector::new(
                        snap_to_grid(child_bounds.x, scale) - child_bounds.x,
                        snap_to_grid(child_bounds.y, scale) - child_bounds.y,
                    );
                    renderer.with_translation(snap, |renderer| {
                        self.children[i].as_widget().draw(
                            &tree.children[i],
                            renderer,
                            theme,
                            style,
                            child_layout,
                            cursor,
                            viewport,
                        );
                    });
                }
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let visible = tree.state.downcast_ref::<State>().visible;
        let bounds = layout.bounds();

        let mut overlays = Vec::new();
        // `visible` is ascending and layout children are emitted in that
        // order, so walking the children in index order pairs each visible
        // page with its own layout.
        let mut layouts = layout.children();

        for (i, (child, child_tree)) in self.children.iter_mut().zip(&mut tree.children).enumerate()
        {
            if !visible.contains(&Some(i)) {
                continue;
            }

            let Some(child_layout) = layouts.next() else {
                break;
            };

            // The page a switch is heading for may be laid out entirely
            // off-pager: it is not drawn, so it opens no overlay either.
            if !child_layout.bounds().intersects(&bounds) {
                continue;
            }

            if let Some(overlay) = child.as_widget_mut().overlay(
                child_tree,
                child_layout,
                renderer,
                viewport,
                translation,
            ) {
                overlays.push(overlay);
            }
        }

        if overlays.is_empty() {
            None
        } else {
            Some(overlay::Group::with_children(overlays).overlay())
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Pager<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + TextureRenderer + 'a,
{
    fn from(pager: Pager<'a, Message, Theme, Renderer>) -> Self {
        Self::new(pager)
    }
}

// The harness is software-only (see `test_support`).
#[cfg(all(test, feature = "tiny-skia"))]
mod tests {
    use super::*;
    use crate::test_support::Harness;
    use iced::widget::{button, column, text};
    use iced_animate::widget::shape;
    use iced_core::clipboard;
    use iced_core::time::Instant;
    use iced_core::{Color, Theme};
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        A,
        B,
    }

    fn two_pages<'a>(motion: Option<&Motion>, current: usize) -> crate::Element<'a, Message> {
        let mut pager = Pager::new([
            column![button("a").on_press(Message::A)],
            column![button("b").on_press(Message::B)],
        ])
        .current(current);
        if let Some(motion) = motion {
            pager = pager.motion(motion.clone());
        }
        match motion {
            Some(motion) => motion.host(pager).into(),
            None => pager.into(),
        }
    }

    #[test]
    fn the_initial_current_page_is_honoured() {
        let mut harness = Harness::new(Size::new(300.0, 200.0), two_pages(None, 1));
        harness.click("b");
        assert_eq!(harness.into_messages(), vec![Message::B]);
    }

    /// Builds page 0, rebuilds with page 1 requested (which runs `diff`),
    /// drives `frames` redraws, then clicks where page 1's button sits and
    /// returns the messages produced.
    fn switch_then_click(motion: Option<&Motion>, frames: u64) -> Vec<Message> {
        let size = Size::new(300.0, 200.0);
        let mut harness = Harness::new(size, two_pages(motion, 0)).rebuild(two_pages(motion, 1));
        let start = Instant::now();
        for frame in 1..=frames {
            harness.redraw(start + Duration::from_millis(16 * frame));
        }
        // Page 1's button is laid out at the page's top-left; after the
        // slide the page sits at x = 0.
        harness.click_at(Point::new(12.0, 12.0));
        harness.into_messages()
    }

    #[test]
    fn without_an_engine_the_switch_is_immediate() {
        assert_eq!(switch_then_click(None, 1), vec![Message::B]);
    }

    #[test]
    fn with_an_engine_the_slide_settles_and_clicks_land_on_the_new_page() {
        let motion = Motion::new();
        assert_eq!(switch_then_click(Some(&motion), 120), vec![Message::B]);
    }

    #[test]
    fn mid_slide_the_outgoing_page_is_still_where_it_is_drawn() {
        let motion = Motion::new();
        // Two frames in, page 0 has moved only a few pixels: its button is
        // still under the cursor and gets the click. Hit-testing follows the
        // drawn position.
        assert_eq!(switch_then_click(Some(&motion), 2), vec![Message::A]);
    }

    type TestPager<'a> = Pager<'a, (), Theme, crate::Renderer>;

    fn pages<'a>(n: usize) -> Vec<crate::Element<'a, ()>> {
        (0..n).map(|i| text(i).into()).collect()
    }

    fn state(tree: &Tree) -> &State {
        tree.state.downcast_ref::<State>()
    }

    fn tree_of(pager: &TestPager<'_>) -> Tree {
        Tree::new(pager as &dyn Widget<(), Theme, crate::Renderer>)
    }

    fn limits(width: f32, height: f32) -> Limits {
        Limits::new(Size::ZERO, Size::new(width, height))
    }

    fn viewport() -> Rectangle {
        Rectangle::with_size(Size::new(300.0, 200.0))
    }

    #[test]
    fn visible_indices_are_ascending_in_range_and_deduplicated() {
        let cam = |first, second| Camera {
            first,
            second,
            position: first as f32,
        };
        assert_eq!(
            visible_indices(cam(2, 2), Some(0), 3),
            [Some(0), Some(2), None]
        );
        assert_eq!(
            visible_indices(cam(0, 1), Some(1), 3),
            [Some(0), Some(1), None]
        );
        assert_eq!(
            visible_indices(cam(1, 2), Some(0), 3),
            [Some(0), Some(1), Some(2)]
        );
        assert_eq!(
            visible_indices(cam(0, 0), Some(9), 3),
            [Some(0), None, None]
        );
        assert_eq!(visible_indices(cam(0, 0), None, 0), [None, None, None]);
    }

    #[test]
    fn current_is_clamped_to_the_page_count() {
        let tree = tree_of(&Pager::new(pages(2)).current(7));
        assert_eq!(state(&tree).switch.current, 1);
        let tree = tree_of(&Pager::new(pages(0)).current(3));
        assert_eq!(state(&tree).switch.current, 0);
    }

    #[test]
    fn removing_pages_while_the_last_is_current_does_not_panic() {
        let three: TestPager<'_> = Pager::new(pages(3)).current(2);
        let mut tree = tree_of(&three);
        assert_eq!(state(&tree).switch.current, 2);

        // Rebuild with fewer pages and a different current index: the stale
        // `current` must be clamped before it indexes the page list.
        let two: TestPager<'_> = Pager::new(pages(2)).current(0);
        two.diff(&mut tree);
        assert_eq!(state(&tree).switch.current, 1);
        assert_eq!(state(&tree).switch.pending, Some(0));
    }

    #[test]
    fn the_slide_track_survives_idle_rebuilds() {
        let motion = Motion::new();
        let build = |current: usize| -> TestPager<'_> {
            Pager::new(pages(2)).current(current).motion(motion.clone())
        };

        let mut tree = tree_of(&build(0));
        build(1).diff(&mut tree);

        let start = Instant::now();
        let _ = motion.tick(start);
        for frame in 1..=200 {
            let _ = motion.tick(start + Duration::from_millis(16 * frame));
        }
        assert!(!state(&tree).is_sliding(), "the first slide has settled");

        // Four idle rebuilds with garbage collection in between.
        for _ in 0..4 {
            motion.end_build();
            build(1).diff(&mut tree);
            motion.collect();
        }

        // Switching again must still slide rather than jump.
        build(0).diff(&mut tree);
        // The engine has been at rest, so the first frame after the switch
        // restarts its clock; the one after it is the first to move.
        let _ = motion.tick(start + Duration::from_millis(16 * 202));
        let _ = motion.tick(start + Duration::from_millis(16 * 203));
        let position = state(&tree).position();
        assert!(
            position > 0.0 && position < 1.0,
            "still sliding after idle rebuilds: {position}"
        );
    }

    #[test]
    fn a_custom_curve_is_used_for_the_slide() {
        use iced_animate::curves::SMOOTH;
        let motion = Motion::new();
        let mut tree = tree_of(&Pager::new(pages(2)).motion(motion.clone()).curve(SMOOTH));
        Pager::new(pages(2))
            .current(1)
            .motion(motion.clone())
            .curve(SMOOTH)
            .diff(&mut tree);
        let start = Instant::now();
        let _ = motion.tick(start);
        let _ = motion.tick(start + Duration::from_millis(16));
        let position = state(&tree).position();
        assert!(
            position > 0.0 && position < 1.0,
            "sliding under SMOOTH: {position}"
        );
    }

    #[test]
    fn a_switch_lays_out_the_target_page_so_operations_reach_it() {
        let motion = Motion::new();
        let build = |current: usize| -> TestPager<'_> {
            Pager::new(pages(3)).current(current).motion(motion.clone())
        };
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(0));
        let mut pager = build(2);
        pager.diff(&mut tree);
        let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert!(
            state(&tree).visible.contains(&Some(2)),
            "the pending page is laid out: {:?}",
            state(&tree).visible
        );
        assert_eq!(node.children().len(), 2, "the current page and the target");
    }

    #[test]
    fn visible_is_ascending_so_layout_children_pair_with_indices_backwards_too() {
        let motion = Motion::new();
        let build = |current: usize| -> TestPager<'_> {
            Pager::new(pages(3)).current(current).motion(motion.clone())
        };
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(2));
        let mut pager = build(0);
        pager.diff(&mut tree);
        let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));

        assert_eq!(state(&tree).visible, [Some(0), Some(2), None]);
        let layout = Layout::new(&node);
        let pairs: Vec<(usize, f32)> = visible_pages(state(&tree).visible, layout)
            .map(|(i, l)| (i, l.bounds().x))
            .collect();
        // Page 0 sits two page-widths to the left of page 2.
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, 0);
        assert_eq!(pairs[1].0, 2);
        assert!((pairs[1].1 - pairs[0].1 - 600.0).abs() < 1e-3, "{pairs:?}");
    }

    #[test]
    fn arriving_invalidates_the_layout_once() {
        let motion = Motion::new();
        let build = |current: usize| -> TestPager<'_> {
            Pager::new(pages(2)).current(current).motion(motion.clone())
        };
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(0));
        let mut pager = build(1);
        pager.diff(&mut tree);
        let start = Instant::now();
        let _ = motion.tick(start);
        for frame in 1..=200 {
            let _ = motion.tick(start + Duration::from_millis(16 * frame));
        }
        assert!(!state(&tree).is_sliding());

        let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        let mut messages: Vec<()> = Vec::new();
        let mut shell = Shell::new(&mut messages);
        pager.update(
            &mut tree,
            &Event::Window(window::Event::RedrawRequested(start)),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut shell,
            &viewport(),
        );
        assert_eq!(state(&tree).switch.current, 1);
        assert!(
            shell.is_layout_invalid(),
            "the pending page must leave the layout"
        );
    }

    #[test]
    fn a_non_redraw_event_does_not_commit_the_switch() {
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&Pager::new(pages(2)));
        let mut pager: TestPager<'_> = Pager::new(pages(2)).current(1);
        pager.diff(&mut tree);
        assert_eq!(state(&tree).switch.pending, Some(1));
        let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        let mut messages: Vec<()> = Vec::new();
        let mut shell = Shell::new(&mut messages);
        pager.update(
            &mut tree,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: Point::ORIGIN,
            }),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut shell,
            &viewport(),
        );
        assert_eq!(
            state(&tree).switch.pending,
            Some(1),
            "only a redraw commits the switch"
        );
    }

    #[test]
    fn a_slide_records_each_visible_page_once() {
        let motion = Motion::new();
        let build = |current: usize| -> TestPager<'_> {
            Pager::new(pages(2)).current(current).motion(motion.clone())
        };
        let mut renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(0));
        let mut pager = build(1);
        pager.diff(&mut tree);

        let start = Instant::now();
        let _ = motion.tick(start);
        let style = renderer::Style {
            text_color: Color::BLACK,
        };
        let mut frame = 0;
        loop {
            frame += 1;
            assert!(frame < 600, "the slide never settled");
            let now = start + Duration::from_millis(16 * frame);
            let _ = motion.tick(now);
            let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));
            let mut messages: Vec<()> = Vec::new();
            let mut shell = Shell::new(&mut messages);
            pager.update(
                &mut tree,
                &Event::Window(window::Event::RedrawRequested(now)),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard::Null,
                &mut shell,
                &viewport(),
            );
            iced_core::Renderer::reset(&mut renderer, viewport());
            pager.draw(
                &tree,
                &mut renderer,
                &Theme::Light,
                &style,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &viewport(),
            );
            if !state(&tree).is_sliding() {
                break;
            }
        }
        let counts: Vec<u64> = state(&tree)
            .pages
            .iter()
            .map(|p| p.cache.record_count())
            .collect();
        assert_eq!(
            counts,
            vec![1, 1],
            "each page is recorded exactly once per slide"
        );
    }

    #[test]
    fn max_height_and_unbounded_width_are_handled() {
        let renderer = crate::testing::headless_tiny_skia();
        let mut pager: TestPager<'_> = Pager::new(pages(2)).max_height(5.0);
        let mut tree = tree_of(&pager);
        let node = pager.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert!(node.size().height <= 5.0, "clamped: {}", node.size().height);

        let node = pager.layout(&mut tree, &renderer, &limits(f32::INFINITY, 200.0));
        assert!(
            node.size().width.is_finite() && node.size().width > 0.0,
            "{:?}",
            node.size()
        );
    }

    #[test]
    fn shrink_takes_the_width_of_the_visible_page() {
        let renderer = crate::testing::headless_tiny_skia();
        let squares = || -> Vec<crate::Element<'static, ()>> {
            vec![
                shape().width(40.0).height(10.0).fill(Color::BLACK).into(),
                shape().width(90.0).height(10.0).fill(Color::BLACK).into(),
            ]
        };
        let mut narrow: TestPager<'_> = Pager::new(squares()).width(Length::Shrink);
        let mut tree = tree_of(&narrow);
        let node = narrow.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert_eq!(node.size().width, 40.0, "page 0 is the only visible page");
        assert_eq!(
            narrow.size().width,
            Length::Shrink,
            "reported to the parent as well"
        );

        let mut wide: TestPager<'_> = Pager::new(squares()).width(Length::Shrink).current(1);
        let mut tree = tree_of(&wide);
        let node = wide.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert_eq!(node.size().width, 90.0);
    }

    #[test]
    fn an_empty_pager_lays_out_and_draws_nothing() {
        let mut harness = Harness::new(
            Size::new(300.0, 200.0),
            Pager::<'_, ()>::new(Vec::<crate::Element<'_, ()>>::new()),
        );
        harness.frame(Instant::now());
    }
}
