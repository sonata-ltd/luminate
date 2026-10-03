//! [`Luminate::pager`].

use iced_animate::curves;
use iced_texture_cache::Pager;
use iced_texture_cache::PixelSnap;

use crate::Element;
use crate::descriptor;
use crate::luminate::Luminate;

impl Luminate {
    /// Builds a sliding page stack on the kit's engine.
    #[must_use]
    pub fn pager<'a, M: Clone + 'a>(&self, descriptor: descriptor::Pager<'a, M>) -> Element<'a, M> {
        let descriptor::Pager {
            pages,
            current,
            width,
            max_height,
        } = descriptor;

        let mut pager = Pager::new(pages)
            .current(current)
            .motion(self.motion.clone())
            .width(width)
            .curve(curves::sharp::STRUCTURAL)
            .pixel_snap(PixelSnap::LayoutOnly);

        if let Some(max_height) = max_height {
            pager = pager.max_height(max_height);
        }

        pager.into()
    }
}
