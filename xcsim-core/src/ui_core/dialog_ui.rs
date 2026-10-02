xcsim_core_l10n::tl_file!("dialog");

use super::{DRectButton, RectButton, Scroll, Ui};
use crate::{core::BOLD_FONT, ext::RectExt, scene::show_message};
use anyhow::Error;
use macroquad::prelude::*;

const WIDTH_RADIO: f32 = 0.5;
const HEIGHT_RATIO: f32 = 0.85;

const MSG_PAD: f32 = 0.06;

type DialogListener = dyn FnMut(&mut Dialog, i32) -> bool;

#[must_use]
pub struct Dialog {
    title: String,
    message: String,
    buttons: Vec<String>,

    listener: Option<Box<DialogListener>>,

    text_btn: RectButton,

    h: Option<f32>,
    msg_size: f32,

    scroll: Scroll,
    window_rect: Option<Rect>,
    rect_buttons: Vec<DRectButton>,
}

impl Default for Dialog {
    fn default() -> Self {
        Self {
            title: tl!("notice").to_string(),
            message: String::new(),
            buttons: vec![tl!("ok").to_string()],
            listener: None,

            text_btn: RectButton::new(),

            h: None,
            msg_size: 0.46,

            scroll: Scroll::new(),
            window_rect: None,
            rect_buttons: vec![DRectButton::new()],
        }
    }
}

impl Dialog {
    pub fn simple(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn plain(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn error(error: Error) -> Self {
        let error = format!("{error:?}");
        Self {
            title: tl!("error").to_string(),
            message: error.clone(),
            buttons: vec![tl!("error-copy").to_string(), tl!("ok").to_string()],
            listener: Some(Box::new(move |_dialog, pos| {
                if pos == 0 {
                    unsafe { get_internal_gl() }.quad_context.clipboard_set(&error);
                    show_message(tl!("error-copied")).ok();
                }
                false
            })),

            rect_buttons: vec![DRectButton::new(); 2],
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.set_message(message);
        self
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn buttons(mut self, buttons: Vec<String>) -> Self {
        self.set_buttons(buttons);
        self
    }

    pub fn set_buttons(&mut self, buttons: Vec<String>) {
        self.buttons = buttons;
        self.rect_buttons = vec![DRectButton::new(); self.buttons.len()];
    }

    pub fn listener(mut self, f: impl FnMut(&mut Dialog, i32) -> bool + 'static) -> Self {
        self.listener = Some(Box::new(f));
        self
    }

    pub fn show(self) {
        crate::scene::DIALOG.with(|it| *it.borrow_mut() = Some(self));
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.scroll.touch(touch, t);
        let mut exit = false;
        for (index, btn) in self.rect_buttons.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                if let Some(mut listener) = self.listener.take() {
                    if !listener(self, index as i32) {
                        exit = true;
                    }
                    self.listener = Some(listener);
                    break;
                } else {
                    exit = true;
                    break;
                }
            }
        }
        if self.text_btn.touch(touch) {
            if let Some(mut listener) = self.listener.take() {
                if !listener(self, -2) {
                    exit = true;
                }
                self.listener = Some(listener);
            }
        }
        if exit {
            return false;
        }

        if self
            .window_rect
            .is_none_or(|rect| rect.contains(touch.position) || touch.phase != TouchPhase::Started)
        {
            true
        } else {
            if let Some(mut listener) = self.listener.take() {
                if listener(self, -1) {
                    return true;
                }
                self.listener = Some(listener);
            }
            false
        }
    }

    pub fn update(&mut self, t: f32) {
        self.scroll.update(t);
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), Color::new(0., 0., 0., 0.45));

        let mh = ui.top * 2. * HEIGHT_RATIO;
        let pad = 0.03_f32;
        let title_h = 0.10_f32;
        let bh = 0.075_f32;
        let btn_w = 0.20_f32;
        let btn_gap = 0.012_f32;
        let btn_pad = 0.018_f32;

        if self.h.is_none() {
            const BASE_SIZE: f32 = 0.46;
            let max_msg_w = 2. * WIDTH_RADIO - pad * 2.;

            let avail_h = mh - title_h - bh - btn_pad * 4. - MSG_PAD;
            let full_h = ui.text(&self.message).size(BASE_SIZE).max_width(max_msg_w).multiline().measure().h;

            self.msg_size = if full_h > avail_h && full_h > 0. {
                (BASE_SIZE * (avail_h * 0.92 / full_h)).clamp(0.24, BASE_SIZE)
            } else {
                BASE_SIZE
            };
            let fitted_h = ui.text(&self.message).size(self.msg_size).max_width(max_msg_w).multiline().measure().h;
            self.h = Some((fitted_h + MSG_PAD + title_h + bh + btn_pad * 4.).min(mh));
        }
        let msg_size = self.msg_size;
        let mut wr = Rect::new(0., 0., 2. * WIDTH_RADIO, self.h.unwrap());
        wr.x = -wr.w / 2.;
        wr.y = -wr.h / 2.;
        self.window_rect = Some(ui.rect_to_global(wr));

        let body_bg    = Color::new(0.165, 0.110, 0.180, 1.);
        let accent     = Color::new(0.949, 0.412, 0.580, 1.0);
        let pink_soft  = Color::new(1.0, 0.580, 0.706, 1.0);
        let border_col = Color::new(1.0, 0.776, 0.847, 0.45);
        let dark_text  = Color::new(0.984, 0.973, 0.886, 1.);

        ui.fill_path(&wr.feather(0.014).rounded(0.05), Color::new(0.949, 0.412, 0.580, 0.40));
        ui.fill_path(&wr.rounded(0.04), body_bg);

        let title_r = Rect::new(wr.x, wr.y, wr.w, title_h);
        let title_cy = title_r.center().y;
        ui.text(&self.title)
            .pos(title_r.x + pad, title_cy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.54)
            .color(dark_text)
            .draw_using(&BOLD_FONT);
        ui.fill_path(&Rect::new(wr.x + pad, wr.y + title_h - 0.006, 0.14, 0.006).rounded(0.003), pink_soft);

        let msg_y = wr.y + title_h + btn_pad;
        let msg_h = wr.h - title_h - bh - btn_pad * 4.;
        self.scroll.size((wr.w - pad * 2., msg_h));
        ui.scope(|ui| {
            ui.dx(wr.x + pad);
            ui.dy(msg_y);
            self.scroll.render(ui, |ui| {
                let r = ui
                    .text(&self.message)
                    .pos(0., 0.)
                    .anchor(0., 0.)
                    .size(msg_size)
                    .max_width(wr.w - pad * 2.)
                    .multiline()
                    .color(dark_text)
                    .draw();
                self.text_btn.set(ui, r);
                (r.w, r.h + MSG_PAD)
            });
        });

        let n = self.buttons.len();
        let by = wr.bottom() - btn_pad - bh;
        let mut bx = wr.right() - btn_pad;
        for i in (0..n).rev() {
            bx -= btn_w;
            let r = Rect::new(bx, by, btn_w, bh);
            let is_primary = i == n - 1;
            let text = self.buttons[i].clone();
            let btn = &mut self.rect_buttons[i];
            if is_primary {
                btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, accent);
                    ui.text(&text)
                        .pos(r.center().x, r.center().y)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.42)
                        .color(WHITE)
                        .draw();
                });
            } else {
                btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, body_bg);
                    ui.stroke_path(&path, 0.005, border_col);
                    ui.text(&text)
                        .pos(r.center().x, r.center().y)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.42)
                        .color(dark_text)
                        .draw();
                });
            }
            bx -= btn_gap;
        }
    }
}
