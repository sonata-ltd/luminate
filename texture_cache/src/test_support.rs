//! In-crate test harness: a `UserInterface` on the headless tiny-skia
//! renderer, so widget tests are deterministic about which backend they run
//! on (`iced_test::Simulator` picks its backend from `ICED_TEST_BACKEND`).

use iced_core::renderer::Headless;
use iced_core::time::Instant;
use iced_core::widget::{Operation, operation};
use iced_core::{
    Color, Element, Event, Point, Rectangle, Size, Theme, clipboard, mouse, renderer, window,
};
use iced_runtime::user_interface::{self, UserInterface};
use iced_test::selector::Selector;

use crate::Renderer;

/// A widget tree driven frame by frame on the software backend.
pub(crate) struct Harness<'a, Message> {
    ui: UserInterface<'a, Message, Theme, Renderer>,
    renderer: Renderer,
    size: Size,
    cursor: mouse::Cursor,
    messages: Vec<Message>,
}

impl<'a, Message> Harness<'a, Message> {
    /// Builds `element` in a window of `size` logical pixels (scale 1.0).
    pub(crate) fn new(
        size: Size,
        element: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        let mut renderer = crate::testing::headless_tiny_skia();
        let ui = UserInterface::build(
            element,
            size,
            user_interface::Cache::default(),
            &mut renderer,
        );
        Self {
            ui,
            renderer,
            size,
            cursor: mouse::Cursor::Unavailable,
            messages: Vec::new(),
        }
    }

    /// Rebuilds the tree from a new element, diffing against the old state
    /// exactly like a view rebuild.
    pub(crate) fn rebuild(
        mut self,
        element: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        let cache = self.ui.into_cache();
        self.ui = UserInterface::build(element, self.size, cache, &mut self.renderer);
        self
    }

    /// Feeds `events` through `update` with the current cursor.
    pub(crate) fn update(&mut self, events: &[Event]) {
        let _ = self.ui.update(
            events,
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut self.messages,
        );
    }

    /// One `RedrawRequested` at `now` (no draw).
    pub(crate) fn redraw(&mut self, now: Instant) {
        self.update(&[Event::Window(window::Event::RedrawRequested(now))]);
    }

    /// Draws the tree into the renderer (records textures as a real frame would).
    pub(crate) fn draw(&mut self) {
        let viewport = Rectangle::with_size(self.size);
        iced_core::Renderer::reset(&mut self.renderer, viewport);
        self.ui.draw(
            &mut self.renderer,
            &Theme::Light,
            &renderer::Style {
                text_color: Color::BLACK,
            },
            self.cursor,
        );
    }

    /// A full frame: `RedrawRequested` then `draw`.
    pub(crate) fn frame(&mut self, now: Instant) {
        self.redraw(now);
        self.draw();
    }

    /// Points the cursor at `point` and simulates a press + release there.
    pub(crate) fn click_at(&mut self, point: Point) {
        self.cursor = mouse::Cursor::Available(point);
        let events: Vec<Event> = iced_test::simulator::click().collect();
        self.update(&events);
    }

    /// Runs `operation` over the tree.
    pub(crate) fn operate<T>(&mut self, operation: &mut dyn Operation<T>) {
        self.ui
            .operate(&self.renderer, &mut operation::black_box(operation));
    }

    /// Clicks the centre of the first text widget reading `label`.
    ///
    /// # Panics
    ///
    /// If no such text is laid out.
    pub(crate) fn click(&mut self, label: &str) {
        let mut find = Selector::find(label);
        self.ui
            .operate(&self.renderer, &mut operation::black_box(&mut find));
        let operation::Outcome::Some(Some(target)) = find.finish() else {
            panic!("no text {label:?} on screen")
        };
        let bounds = target.visible_bounds().expect("the text is visible");
        self.click_at(bounds.center());
    }

    /// Draws, then reads the frame back at `scale_factor`. Like a window
    /// `present`, this also sets the renderer's scale factor for the frames
    /// that follow.
    pub(crate) fn screenshot(&mut self, scale_factor: f32) -> Screenshot {
        self.draw();
        let width = (self.size.width * scale_factor).round() as u32;
        let height = (self.size.height * scale_factor).round() as u32;
        let rgba = self
            .renderer
            .screenshot(Size::new(width, height), scale_factor, Color::WHITE);
        Screenshot { rgba, width }
    }

    pub(crate) fn into_messages(self) -> Vec<Message> {
        self.messages
    }
}

/// RGBA8 pixels of one frame.
pub(crate) struct Screenshot {
    rgba: Vec<u8>,
    width: u32,
}

impl Screenshot {
    /// The `[r, g, b, a]` of the pixel at physical `(x, y)`.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * self.width + x) * 4) as usize;
        let px = &self.rgba[start..start + 4];
        [px[0], px[1], px[2], px[3]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_animate::widget::shape;

    #[test]
    fn a_red_square_lands_where_it_is_laid_out() {
        let square: Element<'_, (), Theme, Renderer> = shape()
            .width(20.0)
            .height(20.0)
            .fill(Color::from_rgb(1.0, 0.0, 0.0))
            .into();
        let mut harness = Harness::new(Size::new(50.0, 50.0), square);
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(&shot.pixel(5, 5)[..3], &[255, 0, 0]);
        assert_eq!(&shot.pixel(40, 40)[..3], &[255, 255, 255]);
    }
}
