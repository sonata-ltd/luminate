//! Measure where a target is drawn; nested scrollables are never moved.
//!
//! Layout bounds below a nested scrollable ignore its scroll offset, so the
//! translations of the scrollables enclosing the target are subtracted.
use iced::advanced::widget::{Operation, operation::Scrollable};
use iced::{Rectangle, Vector, widget::Id};
#[derive(Default)]
pub(super) struct Target<'a> {
    id: Option<&'a Id>,
    viewport: Option<Rectangle>,
    content: Rectangle,
    bounds: Option<Rectangle>,
    /// The translation of a nested scrollable just met, for the `traverse`
    /// into its content that follows.
    pending: Option<Vector>,
    /// Translations of the nested scrollables around the current widget.
    nested: Vec<Vector>,
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
        let translation = self.pending.take().unwrap_or(Vector::ZERO);
        self.nested.push(translation);
        visit(self);
        self.nested.pop();
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        if self.viewport.is_none() {
            self.viewport = Some(bounds);
            self.content = content;
        } else {
            self.pending = Some(translation);
        }
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if self.id.is_some() && id == self.id {
            let shift = self
                .nested
                .iter()
                .fold(Vector::ZERO, |sum, translation| sum + *translation);
            self.bounds = Some(bounds - shift);
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
