//! Telling a touch tap from the start of a scroll gesture.

use iced::{Event, Point, touch};

/// How far a finger may travel, in logical pixels, and still count as a tap.
const SLOP: f32 = 8.0;

/// A finger press that becomes a tap when lifted without dragging.
///
/// Widgets inside a scrollable must not capture `FingerPressed`: the
/// scrollable only starts a touch gesture for presses its content left alone.
/// They record the press here instead and act on the lift.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Tap {
    start: Option<(touch::Finger, Point)>,
}

impl Tap {
    pub(crate) fn press(&mut self, id: touch::Finger, position: Point) {
        self.start = Some((id, position));
    }

    pub(crate) fn cancel(&mut self) {
        self.start = None;
    }

    /// Follows `event`; returns `true` when it cancelled the pending tap
    /// (the finger dragged past the slop or was lost).
    pub(crate) fn track(&mut self, event: &Event) -> bool {
        let Some((finger, start)) = self.start else {
            return false;
        };

        let cancelled = match event {
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                *id == finger && position.distance(start) > SLOP
            }
            Event::Touch(touch::Event::FingerLost { id, .. }) => *id == finger,
            _ => false,
        };

        if cancelled {
            self.start = None;
        }
        cancelled
    }

    /// Ends the tap on `event`, returning `true` when it was a lift of the
    /// pressed finger that stayed within the slop.
    pub(crate) fn lift(&mut self, event: &Event) -> bool {
        match (event, self.start) {
            (Event::Touch(touch::Event::FingerLifted { id, .. }), Some((finger, _)))
                if *id == finger =>
            {
                self.start = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: touch::Finger = touch::Finger(1);

    fn moved(x: f32) -> Event {
        Event::Touch(touch::Event::FingerMoved {
            id: F,
            position: Point::new(x, 0.0),
        })
    }

    fn lifted() -> Event {
        Event::Touch(touch::Event::FingerLifted {
            id: F,
            position: Point::ORIGIN,
        })
    }

    #[test]
    fn a_still_finger_taps() {
        let mut tap = Tap::default();
        tap.press(F, Point::ORIGIN);
        assert!(!tap.track(&moved(3.0)));
        assert!(tap.lift(&lifted()));
    }

    #[test]
    fn a_drag_is_not_a_tap() {
        let mut tap = Tap::default();
        tap.press(F, Point::ORIGIN);
        assert!(tap.track(&moved(40.0)));
        assert!(!tap.lift(&lifted()));
    }
}
