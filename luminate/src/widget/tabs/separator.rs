use iced_animate::{Anim, Motion, MotionKey, curves::QUICK};

#[derive(Debug)]
pub(crate) struct SeparatorState {
    key: MotionKey,
    opacity: Anim<f32>,
    target: Option<f32>,
}

impl SeparatorState {
    pub(crate) fn new() -> Self {
        Self {
            key: MotionKey::unique(),
            opacity: Anim::constant(1.0),
            target: None,
        }
    }

    pub(crate) fn retarget(&mut self, motion: Option<&Motion>, target: f32) {
        let target = target.clamp(0.0, 1.0);

        let changed = self
            .target
            .is_none_or(|t| (t - target).abs() > f32::EPSILON);

        if !changed {
            return;
        }

        self.target = Some(target);

        self.opacity = match motion {
            Some(motion) => motion.to(self.key, QUICK, target),
            None => Anim::constant(target),
        };
    }

    pub(crate) fn value(&self) -> f32 {
        self.opacity.get().clamp(0.0, 1.0)
    }
}
