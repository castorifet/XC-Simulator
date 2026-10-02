use macroquad::color::Color;

pub const FIREFLY_PINK: Color = Color::new(1.000, 0.580, 0.706, 1.0);
pub const FIREFLY_PINK_SOFT: Color = Color::new(1.000, 0.776, 0.847, 1.0);
pub const FIREFLY_PINK_DEEP: Color = Color::new(0.949, 0.412, 0.580, 1.0);

pub const FIREFLY_CREAM: Color = Color::new(0.941, 0.945, 0.780, 1.0);
pub const FIREFLY_CREAM_SOFT: Color = Color::new(0.984, 0.973, 0.886, 1.0);

pub const FIREFLY_MINT: Color = Color::new(0.682, 0.804, 0.780, 1.0);
pub const FIREFLY_MINT_DEEP: Color = Color::new(0.435, 0.612, 0.612, 1.0);
pub const FIREFLY_TEAL: Color = Color::new(0.259, 0.541, 0.569, 1.0);
pub const FIREFLY_TEAL_DEEP: Color = Color::new(0.176, 0.384, 0.416, 1.0);

pub const FIREFLY_GOLD: Color = Color::new(0.961, 0.792, 0.502, 1.0);
pub const FIREFLY_PEACH: Color = Color::new(0.992, 0.847, 0.694, 1.0);

pub const FIREFLY_PLUM: Color = Color::new(0.196, 0.122, 0.196, 1.0);
pub const FIREFLY_PLUM_DEEP: Color = Color::new(0.133, 0.071, 0.137, 1.0);

#[inline]
pub fn panel() -> Color {
    Color::new(0.165, 0.110, 0.180, 0.93)
}

#[inline]
pub fn panel_soft() -> Color {
    Color::new(0.243, 0.165, 0.255, 0.84)
}

#[inline]
pub fn top_bar() -> Color {
    Color::new(0.122, 0.071, 0.137, 0.97)
}

#[inline]
pub fn accent_line() -> Color {
    FIREFLY_PINK_DEEP
}

#[inline]
pub fn accent_glow() -> Color {
    Color::new(1.000, 0.580, 0.706, 0.28)
}

#[inline]
pub fn mint_accent() -> Color {
    FIREFLY_MINT
}

#[inline]
pub fn title_text() -> Color {
    FIREFLY_CREAM_SOFT
}

#[inline]
pub fn subtitle_text() -> Color {
    Color::new(1.000, 0.776, 0.847, 0.85)
}

#[inline]
pub fn pink_overlay(alpha: f32) -> Color {
    Color::new(1.000, 0.580, 0.706, alpha)
}

#[inline]
pub fn cream_text(alpha: f32) -> Color {
    Color::new(0.984, 0.973, 0.886, alpha)
}

#[inline]
pub fn mint_overlay(alpha: f32) -> Color {
    Color::new(0.682, 0.804, 0.780, alpha)
}

#[inline]
pub fn plum_overlay(alpha: f32) -> Color {
    Color::new(0.133, 0.071, 0.137, alpha)
}

#[inline]
pub fn cream_white() -> Color {
    Color::new(0.984, 0.973, 0.886, 1.0)
}

#[inline]
pub fn plum_black() -> Color {
    Color::new(0.094, 0.043, 0.094, 1.0)
}
