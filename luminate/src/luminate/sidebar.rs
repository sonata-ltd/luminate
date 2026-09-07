//! [`Luminate::sidebar`].

use crate::Element;
use crate::descriptor::Sidebar;
use crate::luminate::Luminate;
use crate::widget::sidebar::Sidebar as Widget;

impl Luminate {
    /// Builds a collapsible sidebar on the kit's engine, with every metric
    /// (collapsed size, header size, icon size, padding, spacing) from the
    /// sidebar tokens and the colours from the theme's `sidebar::Catalog`.
    #[must_use]
    pub fn sidebar<'a, M: Clone + 'a>(&self, descriptor: Sidebar<'a, M>) -> Element<'a, M> {
        let tokens = self.theme.sidebar;

        let mut sidebar = Widget::with_children(descriptor.children)
            .motion(self.motion.clone())
            .width(descriptor.width)
            .height(descriptor.height)
            .axis(descriptor.axis)
            .collapsed(descriptor.collapsed)
            .show_toggle(descriptor.show_toggle)
            .collapsed_size(tokens.collapsed_size)
            .header_size(tokens.header_size)
            .icon_size(tokens.icon_size)
            .padding(tokens.padding)
            .spacing(tokens.spacing(descriptor.axis));

        if let Some(on_toggle) = descriptor.on_toggle {
            sidebar = sidebar.on_toggle(on_toggle);
        }

        sidebar.into()
    }
}
