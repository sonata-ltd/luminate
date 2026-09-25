//! Caller-owned invalidation handle for a cached texture.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Unique identity of a [`TextureCache`]; shared by all its clones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureCacheId(u64);

impl std::fmt::Display for TextureCacheId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cache#{}", self.0)
    }
}

// Orderings are `Relaxed` throughout: the whole record/composite pipeline is
// single-threaded by design, and the flag guards no other memory. An
// `invalidate()` issued from another thread (a subscription) is still
// observed by the next frame because atomics are coherent, just not ordered
// against anything else.
#[derive(Debug)]
pub(crate) struct Inner {
    invalidated: AtomicBool,
    /// Bumped by every `invalidate`; widgets compare it against the value
    /// they last propagated to their ancestors.
    generation: AtomicU64,
    records: AtomicU64,
    /// The last record was refused as too large, so the widget is drawing
    /// its content in place, without the warp a texture would carry; its
    /// hit-testing has to know. Cleared by the next record that succeeds.
    uncacheable: AtomicBool,
}

/// A handle to a cached texture. Cloning shares the same cache.
///
/// A new cache starts invalidated so its first draw records. Call
/// [`invalidate`](Self::invalidate) whenever the cached content changed
/// without an event; size and scale changes are detected by the renderer,
/// and [`Cached`](crate::Cached) invalidates on its own when the content
/// reacts to an event.
///
/// # One handle per widget
///
/// A handle identifies **one** texture. Clone it to hold the same handle in
/// several places (application state and a subscription, say), not to share
/// a texture between two widgets: two `Cached` widgets driving the same
/// handle re-record each other whenever their sizes differ, and whichever
/// draws last wins.
///
/// [`Frosted`](crate::Frosted) is the exception that is not one: it only
/// *reads* the texture a `Cached` wrote, never records into it, so a handle
/// driven by one `Cached` and read by any number of panes of glass is
/// exactly the intended use. The rule above is about two *writers* fighting
/// over the same texture; a reader that never writes cannot be one of them,
/// however many of them there are.
///
/// # Memory
///
/// The GPU texture (or CPU pixmap) behind a cache lives as long as *any*
/// handle does; it is released at the start of the next presented frame (or
/// the next headless screenshot) after the last handle is dropped. Store
/// handles where the lifetime of the content is: application state for a
/// long-lived panel, widget state for something that comes and goes.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_texture_cache::{TextureCache, cached};
///
/// struct App { cache: TextureCache }
///
/// impl App {
///     fn view(&self) -> iced_texture_cache::Element<'_, ()> {
///         cached(self.cache.clone(), text("expensive")).into()
///     }
///
///     fn content_changed(&mut self) {
///         // Something changed without an event reaching the subtree.
///         self.cache.invalidate();
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TextureCache {
    id: TextureCacheId,
    inner: Arc<Inner>,
}

impl TextureCache {
    /// Creates a new, invalidated cache with a fresh identity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: TextureCacheId(NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            inner: Arc::new(Inner {
                invalidated: AtomicBool::new(true),
                generation: AtomicU64::new(1),
                records: AtomicU64::new(0),
                uncacheable: AtomicBool::new(false),
            }),
        }
    }

    /// The identity shared by every clone of this handle.
    #[must_use]
    pub fn id(&self) -> TextureCacheId {
        self.id
    }

    /// Marks the cached content stale; the next draw re-records it.
    pub fn invalidate(&self) {
        // Two independent stores: the renderer reads only the flag and
        // `ancestors::propagate` reads only the generation, so their
        // relative order is never observed.
        self.inner.invalidated.store(true, Ordering::Relaxed);
        let _ = self.inner.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether the cache is waiting to be re-recorded.
    #[must_use]
    pub fn is_invalidated(&self) -> bool {
        self.inner.invalidated.load(Ordering::Relaxed)
    }

    /// Number of times [`invalidate`](Self::invalidate) has been called,
    /// plus one for the initial invalidation. Never decreases; consuming the
    /// flag by recording does not change it. Nested caches use it to
    /// propagate an invalidation to their ancestors exactly once.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Relaxed)
    }

    /// Number of times this cache has been rasterized. Diagnostics only.
    #[must_use]
    pub fn record_count(&self) -> u64 {
        self.inner.records.load(Ordering::Relaxed)
    }

    /// Clears the invalidation flag and returns whether it was set.
    ///
    /// For implementors of [`TextureRenderer`](crate::TextureRenderer):
    /// call it once per `record`, before deciding whether to re-record, so
    /// an invalidation is consumed even when the request is uncacheable.
    #[must_use]
    pub fn take_invalidated(&self) -> bool {
        self.inner.invalidated.swap(false, Ordering::Relaxed)
    }

    /// Counts one rasterization (see [`record_count`](Self::record_count)).
    ///
    /// For implementors of [`TextureRenderer`](crate::TextureRenderer):
    /// call it once per `Record::Fresh`.
    pub fn note_record(&self) {
        let _ = self.inner.records.fetch_add(1, Ordering::Relaxed);
        self.inner.uncacheable.store(false, Ordering::Relaxed);
    }

    /// Whether the last record was refused as too large for a texture, so
    /// the content is being drawn in place; see [`Record::Uncacheable`].
    ///
    /// [`Record::Uncacheable`]: crate::Record::Uncacheable
    pub(crate) fn is_uncacheable(&self) -> bool {
        self.inner.uncacheable.load(Ordering::Relaxed)
    }

    /// Notes that a record was refused as too large.
    pub(crate) fn note_uncacheable(&self) {
        self.inner.uncacheable.store(true, Ordering::Relaxed);
    }

    pub(crate) fn liveness(&self) -> Weak<Inner> {
        Arc::downgrade(&self.inner)
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for TextureCache {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for TextureCache {}

impl std::hash::Hash for TextureCache {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cache_starts_invalidated() {
        let cache = TextureCache::new();
        assert!(cache.is_invalidated());
        assert_eq!(cache.record_count(), 0);
    }

    #[test]
    fn take_invalidated_clears_flag_once() {
        let cache = TextureCache::new();
        assert!(cache.take_invalidated());
        assert!(!cache.take_invalidated());
        assert!(!cache.is_invalidated());
    }

    #[test]
    fn clones_share_state_and_ids_are_unique() {
        let a = TextureCache::new();
        let b = a.clone();
        let c = TextureCache::new();
        let _ = a.take_invalidated();
        assert!(!b.is_invalidated());
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.id(), b.id());
    }

    #[test]
    fn liveness_dies_when_all_handles_drop() {
        let a = TextureCache::new();
        let weak = a.liveness();
        let b = a.clone();
        drop(a);
        assert!(weak.upgrade().is_some());
        drop(b);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn record_count_is_per_cache() {
        let a = TextureCache::new();
        let b = a.clone();
        a.note_record();
        a.note_record();
        assert_eq!(b.record_count(), 2);
        assert_eq!(TextureCache::new().record_count(), 0);
    }

    #[test]
    fn invalidate_bumps_the_generation_every_time() {
        let cache = TextureCache::new();
        let fresh = cache.generation();
        assert!(fresh >= 1, "a new cache has already been invalidated once");
        cache.invalidate();
        assert_eq!(cache.generation(), fresh + 1);
        // Already invalidated: still bumps, so a pending invalidation of a
        // hidden cache reaches its ancestors exactly once more.
        cache.invalidate();
        assert_eq!(cache.generation(), fresh + 2);
        let _ = cache.take_invalidated();
        assert_eq!(
            cache.generation(),
            fresh + 2,
            "consuming the flag is not a change"
        );
    }

    #[test]
    fn id_displays_with_its_number_and_clones_share_it() {
        let a = TextureCache::new();
        let b = a.clone();
        assert_eq!(a.id(), b.id());
        assert!(a.id().to_string().starts_with("cache#"));
        let _: TextureCacheId = a.id();
    }
}
