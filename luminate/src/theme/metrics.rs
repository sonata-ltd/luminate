//! Metric helpers the tokens need as `const` (iced's own constructors are
//! not). Crate-private on purpose: a theme author writes the resulting
//! values into the token structs directly.

use iced::Padding;

/// Spacing definitions
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    /// XXS - 2px
    pub xxs: f32,
    /// XS - 4px
    pub xs: f32,
    /// SM - 6px
    pub sm: f32,
    /// MD - 8px
    pub md: f32,
    /// LG - 12px
    pub lg: f32,
    /// XL - 16px
    pub xl: f32,
    /// 2XL - 20px
    pub xl2: f32,
    /// 3XL - 24px
    pub xl3: f32,
    /// 4XL - 32px
    pub xl4: f32,
    /// 5XL - 40px
    pub xl5: f32,
    /// 6XL - 48px
    pub xl6: f32,
    /// 7XL - 64px
    pub xl7: f32,
    /// 8XL - 80px
    pub xl8: f32,
    /// 9XL - 96px
    pub xl9: f32,
    /// 10XL - 128px
    pub xl10: f32,
    /// 11XL - 160xp
    pub xl11: f32,
}

impl Spacing {
    pub const fn new() -> Self {
        Self {
            xxs: 2.0,
            xs: 4.0,
            sm: 6.0,
            md: 8.0,
            lg: 12.0,
            xl: 16.0,
            xl2: 20.0,
            xl3: 24.0,
            xl4: 32.0,
            xl5: 40.0,
            xl6: 48.0,
            xl7: 64.0,
            xl8: 80.0,
            xl9: 96.0,
            xl10: 128.0,
            xl11: 160.0,
        }
    }
}

/// Width definitions
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Width {
    /// XXS - 320px
    pub xxs: f32,
    /// XS - 384px
    pub xs: f32,
    /// SM - 480px
    pub sm: f32,
    /// MD - 560px
    pub md: f32,
    /// LG - 640px
    pub lg: f32,
    /// XL - 768px
    pub xl: f32,
    /// 2XL - 1024px
    pub xl2: f32,
    /// 3XL - 1280px
    pub xl3: f32,
    /// 4XL - 1440px
    pub xl4: f32,
    /// 5XL - 1600px
    pub xl5: f32,
    /// 6XL - 1920px
    pub xl6: f32,
}

impl Width {
    pub const fn new() -> Self {
        Self {
            xxs: 320.0,
            xs: 384.0,
            sm: 480.0,
            md: 560.0,
            lg: 640.0,
            xl: 768.0,
            xl2: 1024.0,
            xl3: 1280.0,
            xl4: 1440.0,
            xl5: 1600.0,
            xl6: 1920.0,
        }
    }

    pub const fn paragraph() -> f32 {
        720.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radius {
    /// XXS - 2px
    pub xxs: f32,
    /// XS - 4px
    pub xs: f32,
    /// SM - 6px
    pub sm: f32,
    /// MD - 8px
    pub md: f32,
    /// LG - 10px
    pub lg: f32,
    /// XL - 12px
    pub xl: f32,
    /// 2XL - 16px
    pub xl2: f32,
    /// 3XL - 20px
    pub xl3: f32,
    /// 4XL - 24px
    pub xl4: f32,
    /// MAX - 9999px
    pub max: f32,
}

impl Radius {
    pub const fn new() -> Self {
        Self {
            xxs: 2.0,
            xs: 4.0,
            sm: 6.0,
            md: 8.0,
            lg: 10.0,
            xl: 12.0,
            xl2: 16.0,
            xl3: 20.0,
            xl4: 24.0,
            max: 9999.0,
        }
    }
}

/// Symmetric vertical/horizontal padding.
pub(crate) const fn padding_vh(vertical: f32, horizontal: f32) -> Padding {
    Padding {
        top: vertical,
        right: horizontal,
        bottom: vertical,
        left: horizontal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_vh_is_symmetric() {
        let p = padding_vh(7.0, 15.0);
        assert_eq!((p.top, p.bottom, p.left, p.right), (7.0, 7.0, 15.0, 15.0));
    }
}
