//! [`Luminate::header`].

use crate::Element;
use crate::descriptor::Header;
use crate::luminate::Luminate;
use crate::widget::header::Header as Widget;

impl Luminate {
    /// Builds a collapsible header on the kit's engine, with every metric
    /// (collapsed size, toggle-header size, icon size, padding and spacing)
    /// from the header tokens and paint from the theme's `header::Catalog`.
    #[must_use]
    pub fn header<'a, M: Clone + 'a>(&self, descriptor: Header<'a, M>) -> Element<'a, M> {
        let tokens = self.theme.header;

        let mut header = Widget::with_children(descriptor.children)
            .motion(self.motion.clone())
            .width(descriptor.width)
            .height(descriptor.height)
            .axis(descriptor.axis)
            .shadow_side(descriptor.shadow_side)
            .collapsed(descriptor.collapsed)
            .show_toggle(descriptor.show_toggle)
            .collapsed_size(tokens.collapsed_size)
            .header_size(tokens.header_size)
            .icon_size(tokens.icon_size)
            .padding(tokens.padding)
            .spacing(tokens.spacing(descriptor.axis))
            .border_radius(descriptor.border_radius);

        if let Some(on_toggle) = descriptor.on_toggle {
            header = header.on_toggle(on_toggle);
        }

        header.into()
    }
}
