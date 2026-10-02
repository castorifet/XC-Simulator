use arc_swap::ArcSwap;
use macroquad::color::Color;
use once_cell::sync::Lazy;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StyleFont {
    Default,
    Pgr,
}

#[derive(Clone, Default, Debug)]
pub struct ElementStyle {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub anchor_x: Option<f32>,
    pub anchor_y: Option<f32>,
    pub size: Option<f32>,
    pub color: Option<Color>,
    pub font: Option<StyleFont>,
    pub visible: Option<bool>,
}

impl ElementStyle {
    #[inline]
    pub fn pos(&self, dx: f32, dy: f32) -> (f32, f32) {
        (self.x.unwrap_or(dx), self.y.unwrap_or(dy))
    }
    #[inline]
    pub fn anchor(&self, dax: f32, day: f32) -> (f32, f32) {
        (self.anchor_x.unwrap_or(dax), self.anchor_y.unwrap_or(day))
    }
    #[inline]
    pub fn size(&self, d: f32) -> f32 {
        self.size.unwrap_or(d)
    }
    #[inline]
    pub fn visible(&self, d: bool) -> bool {
        self.visible.unwrap_or(d)
    }
    #[inline]
    pub fn font(&self, d: StyleFont) -> StyleFont {
        self.font.unwrap_or(d)
    }
    #[inline]
    pub fn color(&self, base: Color) -> Color {
        match self.color {
            Some(c) => Color::new(c.r, c.g, c.b, c.a * base.a),
            None => base,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct CustomStyle {
    pub enabled: bool,
    pub score: ElementStyle,
    pub combo_number: ElementStyle,
    pub combo: ElementStyle,
    pub accuracy: ElementStyle,
    pub pause: ElementStyle,
    pub bar: ElementStyle,
    pub name: ElementStyle,
    pub level: ElementStyle,
    pub watermark: ElementStyle,
}

impl CustomStyle {
    pub fn empty() -> Self {
        Self::default()
    }
}

pub static CUSTOM_STYLE: Lazy<ArcSwap<CustomStyle>> = Lazy::new(|| ArcSwap::from_pointee(CustomStyle::empty()));

#[inline]
pub fn current() -> Arc<CustomStyle> {
    CUSTOM_STYLE.load_full()
}

#[inline]
pub fn is_enabled() -> bool {
    CUSTOM_STYLE.load().enabled
}

pub fn apply(style: CustomStyle) {
    CUSTOM_STYLE.store(Arc::new(style));
}

pub fn clear() {
    apply(CustomStyle::empty());
}
