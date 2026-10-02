use macroquad::prelude::*;
use nalgebra::Translation2;
use xcsim_core::{
    core::Matrix,
    ext::{RectExt},
    ui::{button_hit, DRectButton, RectButton, Scroll, Ui},
};

const ANIM_DUR: f32 = 0.20;

pub struct Popup {
    scroll: Scroll,
    pub rect: Rect,
    showing: bool,
    options: Vec<(String, RectButton)>,
    selected: usize,
    hovered: Option<usize>,
    pub left: f32,
    pub size: f32,
    pub height: f32,

    anim_start: f32,
    anim_forward: bool,
    changed: bool,
    auto_dismiss: bool,
    pending_dismiss: bool,
    auto_adjust: Option<Rect>,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            rect: Rect::default(),
            showing: false,
            options: Vec::new(),
            selected: usize::MAX,
            hovered: None,
            left: 0.024,
            size: 0.6,
            height: 0.1,
            anim_start: f32::NAN,
            anim_forward: true,
            changed: false,
            auto_dismiss: true,
            pending_dismiss: false,
            auto_adjust: None,
        }
    }

    #[inline]
    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.set_options(options);
        self
    }

    #[inline]
    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[inline]
    pub fn set_options(&mut self, options: Vec<String>) {
        self.options = options.into_iter().map(|it| (it, RectButton::new())).collect();
    }

    #[inline]
    pub fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
    }

    #[inline]
    pub fn set_auto_dismiss(&mut self, auto_dismiss: bool) {
        self.auto_dismiss = auto_dismiss;
    }

    #[inline]
    pub fn set_auto_adjust(&mut self, auto_adjust: Option<Rect>) {
        self.auto_adjust = auto_adjust;
    }

    pub fn set_bottom(&mut self, _bottom: bool) {}

    pub fn rect(&self) -> Rect {
        self.rect
    }

    fn anim_p(&self, t: f32) -> f32 {
        if self.anim_start.is_nan() {
            return if self.showing { 1. } else { 0. };
        }
        let dt = (t - self.anim_start).clamp(0., ANIM_DUR);
        let linear = dt / ANIM_DUR;
        if self.anim_forward {

            1. - (1. - linear).powi(5)
        } else {

            1. - linear.powi(3)
        }
    }

    pub fn transiting(&self) -> bool {
        !self.anim_start.is_nan()
    }

    pub fn show(&mut self, ui: &mut Ui, t: f32, r: Rect) {
        self.rect = ui.rect_to_global(r);
        if let Some(area) = self.auto_adjust {
            self.rect.x = self.rect.x.clamp(area.x, area.right() - self.rect.w);
            self.rect.y = self.rect.y.clamp(area.y, area.bottom() - self.rect.h);
        }
        self.showing = true;
        self.anim_start = t;
        self.anim_forward = true;
    }

    pub fn dismiss(&mut self, t: f32) {
        self.showing = false;
        self.hovered = None;
        self.anim_start = t;
        self.anim_forward = false;
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, alpha: f32) {
        let p = self.anim_p(t);

        let still_visible = p > 0. || self.showing;
        if !still_visible {
            return;
        }

        let r = self.rect;
        self.scroll.size((r.w, r.h));

        let accent   = crate::theme::FIREFLY_PINK_DEEP;
        let body_bg  = Color::new(0.165, 0.110, 0.180, 1.);
        let text_c   = Color::new(0.984, 0.973, 0.886, 1.);
        let sep_c    = Color::new(1.0, 0.776, 0.847, 0.10);
        let border_c = Color::new(1.0, 0.776, 0.847, 0.45);
        let sel_bg   = Color::new(accent.r, accent.g, accent.b, 0.20);
        let sel_bar  = accent;

        ui.abs_scope(|ui| {
            ui.dx(r.x);
            ui.dy(r.y);

            if self.anim_forward {

                let clip_h = (p * r.h).min(r.h);
                ui.alpha(p.min(1.) * alpha, |ui| {
                    ui.scissor(Rect::new(0., 0., r.w, clip_h), |ui| {
                        self.draw_panel(ui, r, body_bg, border_c, sep_c, sel_bg, sel_bar, text_c, accent, alpha);
                    });
                });
            } else {

                let clip_h = (p * r.h).min(r.h);

                ui.alpha(p.min(1.) * alpha, |ui| {
                    ui.scissor(Rect::new(0., 0., r.w, clip_h), |ui| {
                        self.draw_panel(ui, r, body_bg, border_c, sep_c, sel_bg, sel_bar, text_c, accent, alpha);
                    });
                });
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panel(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        body_bg: Color,
        border_c: Color,
        sep_c: Color,
        sel_bg: Color,
        sel_bar: Color,
        text_c: Color,
        accent: Color,
        alpha: f32,
    ) {
        let r = Rect::new(0., 0., r.w, r.h);
        let panel_path = r.rounded(0.022);
        ui.fill_path(&panel_path, body_bg);
        ui.stroke_path(&panel_path, 0.004, border_c);

        let hovered = self.hovered;
        self.scroll.render(ui, |ui| {
            for (id, (opt, btn)) in self.options.iter_mut().enumerate() {
                if id != 0 {
                    ui.fill_rect(Rect::new(0.024, -0.001, r.w - 0.048, 0.0015), sep_c);
                }
                let ir = Rect::new(0., 0., r.w, self.height);
                btn.set(ui, ir);
                let chosen = id == self.selected;
                let is_hovered = hovered == Some(id) && !chosen;
                let pill = Rect::new(ir.x + 0.012, ir.y + 0.006, ir.w - 0.024, ir.h - 0.012);
                if chosen {
                    ui.fill_path(&pill.rounded(0.018), sel_bg);
                    ui.fill_path(&Rect::new(pill.x, pill.y + 0.008, 0.005, pill.h - 0.016).rounded(0.0025), sel_bar);
                } else if is_hovered {
                    ui.fill_path(&pill.rounded(0.018), Color::new(1.0, 0.776, 0.847, 0.10 * alpha));
                }
                ui.text(opt.as_str())
                    .pos(self.left + if chosen { 0.012 } else { 0.006 }, self.height / 2.)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(self.size)
                    .max_width(r.w - self.left * 2.)
                    .color(if chosen { accent } else { text_c })
                    .draw();
                ui.dy(self.height);
            }
            (r.w, self.options.len() as f32 * self.height)
        });
    }

    pub fn update(&mut self, t: f32) {
        if self.showing {
            let old_matrix = self.scroll.matrix();
            let mut transform = Matrix::identity();
            transform *= Translation2::new(self.rect.x, self.rect.y).to_homogeneous();
            if let Some(inv) = transform.try_inverse() {
                self.scroll.set_matrix(Some(inv));
            }
            self.scroll.update(t);
            self.scroll.set_matrix(old_matrix);
        } else {
            self.scroll.update(t);
        }

        if !self.anim_start.is_nan() && t - self.anim_start >= ANIM_DUR {
            self.anim_start = f32::NAN;
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.pending_dismiss {
            if touch.phase == TouchPhase::Ended {
                self.dismiss(t);
                self.pending_dismiss = false;
            }
            return true;
        }
        if self.showing {

            if matches!(touch.phase, TouchPhase::Moved | TouchPhase::Stationary) {
                if self.rect.contains(touch.position) {
                    self.hovered = self.options.iter().enumerate().find_map(|(id, (_, btn))| {
                        if btn.contains(touch.position) { Some(id) } else { None }
                    });
                } else {
                    self.hovered = None;
                }
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.hovered = None;
            }
            if touch.phase != TouchPhase::Started || self.rect.contains(touch.position) {
                if self.scroll.touch(touch, t) {
                    return true;
                }
                if self.rect.contains(touch.position) {
                    for (id, (_, btn)) in self.options.iter_mut().enumerate() {
                        if btn.touch(touch) {
                            button_hit();
                            if self.selected != id {
                                self.selected = id;
                                self.changed = true;
                            }
                            if self.auto_dismiss {
                                self.dismiss(t);
                            }
                            return true;
                        }
                    }
                    return true;
                }
                false
            } else if touch.phase == TouchPhase::Started {
                self.pending_dismiss = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    #[inline]
    pub fn showing(&self) -> bool {
        self.showing
    }

    #[inline]
    pub fn changed(&mut self) -> bool {
        if self.changed {
            self.changed = false;
            true
        } else {
            false
        }
    }
}

pub struct ChooseButton {
    btn: DRectButton,
    popup: Popup,
    width: Option<f32>,
    height: f32,
    need_to_show: bool,
}

impl ChooseButton {
    pub fn new() -> Self {
        Self {
            btn: DRectButton::new(),
            popup: Popup::new(),
            width: None,
            height: 0.34,
            need_to_show: false,
        }
    }

    #[inline]
    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.popup = self.popup.with_options(options);
        self
    }

    #[inline]
    pub fn with_selected(mut self, selected: usize) -> Self {
        self.popup.selected = selected;
        self
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.popup.selected
    }

    #[inline]
    pub fn changed(&mut self) -> bool {
        self.popup.changed()
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) {
        self.btn
            .render_text(ui, r, t, &self.popup.options[self.popup.selected].0, self.popup.size, false);
        if self.need_to_show {
            let pad = 0.007;
            let mut rr = Rect::new(r.x, r.bottom() + pad, self.width.unwrap_or(r.w), self.height);
            let delta = 0.1;
            rr.x -= delta;
            rr.w += delta;
            self.popup.show(ui, t, rr);
            self.need_to_show = false;
        }
    }

    #[inline]
    pub fn render_top(&mut self, ui: &mut Ui, t: f32, alpha: f32) {
        self.popup.render(ui, t, alpha);
    }

    pub fn update(&mut self, t: f32) {
        self.popup.update(t);
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.popup.showing() {
            if self.popup.touch(touch, t) {
                return true;
            }
            self.popup.rect.contains(touch.position)
        } else {
            self.popup.transiting()
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.btn.touch(touch, t) {
            self.need_to_show = true;
            true
        } else {
            false
        }
    }
}
