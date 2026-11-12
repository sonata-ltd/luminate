//! Measure natural target bounds; nested scrollables are never moved.
use iced::advanced::widget::{Operation, operation::Scrollable};
use iced::{Rectangle, Vector, widget::Id};
#[derive(Default)]
pub(super) struct Target<'a> {
    id: Option<&'a Id>,
    viewport: Option<Rectangle>,
    content: Rectangle,
    bounds: Option<Rectangle>,
}
impl<'a> Target<'a> {
    pub(super) fn new(id: Option<&'a Id>) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }
    pub(super) fn offset(&self, reference: f32, margin: f32) -> Option<f32> {
        let viewport = self.viewport?;
        let bounds = self.bounds?;
        let top = bounds.y - self.content.y;
        let bottom = top + bounds.height;
        let margin = margin.min(viewport.height / 4.0);
        let target = if bounds.height > viewport.height - 2.0 * margin || top < reference + margin {
            top - margin
        } else if bottom > reference + viewport.height - margin {
            bottom - viewport.height + margin
        } else {
            reference
        };
        Some(target.clamp(0.0, (self.content.height - viewport.height).max(0.0)))
    }
}
impl Operation for Target<'_> {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        _: Vector,
        _: &mut dyn Scrollable,
    ) {
        if self.viewport.is_none() {
            self.viewport = Some(bounds);
            self.content = content;
        }
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if self.id.is_some() && id == self.id {
            self.bounds = Some(bounds);
        }
    }
    fn text(&mut self, id: Option<&Id>, bounds: Rectangle, _: &str) {
        self.container(id, bounds);
    }
    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        self.container(id, bounds);
    }
    fn custom(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn std::any::Any) {
        self.container(id, bounds);
    }
}
