xcsim_core_l10n::tl_file!("message");

use std::borrow::Cow;

use super::{Page, SharedState};
use crate::{
    client::{recv_raw, Client, Message},
    get_data, get_data_mut, save_data,
};
use anyhow::Result;
use chrono::Local;
use macroquad::prelude::*;
use xcsim_core::{
    ext::{semi_black, RectExt},
    scene::show_error,
    task::Task,
    ui::{DRectButton, RectButton, Scroll, Ui},
};

pub struct MessagePage {
    msgs: Option<Vec<(Message, DRectButton)>>,
    load_task: Option<Task<Result<Vec<Message>>>>,

    index: Option<usize>,

    btns_scroll: Scroll,
    scroll: Scroll,
    close_btn: RectButton,
}

impl MessagePage {
    pub fn new() -> Self {
        Self {
            msgs: None,
            load_task: None,

            index: None,

            btns_scroll: Scroll::new(),
            scroll: Scroll::new(),
            close_btn: RectButton::new(),
        }
    }

    pub fn load(&mut self) {
        if self.load_task.is_some() {
            return;
        }
        let before = self.msgs.as_ref().and_then(|it| it.last().map(|it| it.0.time));
        self.load_task = Some(Task::new(async move {
            let mut req = Client::get("/message/list");
            if let Some(before) = before {
                req = req.query(&[("before", before)]);
            }
            Ok(recv_raw(req).await?.json().await?)
        }));
    }
}

impl Page for MessagePage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.load();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.index.is_some() {
            if self.close_btn.touch(touch) {
                self.index = None;
                return Ok(true);
            }
            if self.scroll.touch(touch, t) {
                return Ok(true);
            }
            if touch.phase == TouchPhase::Started {
                self.index = None;
            }
            return Ok(true);
        }
        if self.load_task.is_none() {
            if self.btns_scroll.touch(touch, t) {
                return Ok(true);
            }
            if let Some(msgs) = &mut self.msgs {
                for (index, item) in msgs.iter_mut().enumerate() {
                    if item.1.touch(touch, t) {
                        if get_data().message_check_time.is_none_or(|it| it < item.0.time) {
                            get_data_mut().message_check_time = Some(item.0.time);
                            save_data()?;
                        }
                        self.index = Some(index);
                        self.scroll.y_scroller.offset = 0.;
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        if self.btns_scroll.y_scroller.pulled_down {
            self.load();
        }
        self.btns_scroll.update(t);
        self.scroll.update(t);
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-msg-fail")));
                    }
                    Ok(val) => {
                        let mt = match &mut self.msgs {
                            None => self.msgs.insert(Vec::new()),
                            Some(x) => x,
                        };
                        mt.extend(val.into_iter().map(|it| (it, DRectButton::new().with_delta(-0.001))));
                    }
                }
                self.load_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let accent    = crate::theme::FIREFLY_PINK_DEEP;
        let card_bg   = Color::new(0.255, 0.130, 0.190, 0.96);
        let card_sel  = Color::new(0.37, 0.185, 0.265, 0.99);
        let dark_text = Color::new(0.984, 0.973, 0.886, 1.);
        let muted     = Color::new(1.0, 0.776, 0.847, 0.66);
        let border_c  = Color::new(1.0, 0.776, 0.847, 0.40);
        let sep_c     = Color::new(1.0, 0.776, 0.847, 0.16);
        let rad       = 0.02_f32;

        let cr = ui.content_rect();

        s.render_fader(ui, |ui| {
            let sr = ui.screen_rect();
            ui.fill_rect(sr, Color::new(0.16, 0.08, 0.13, 0.55));
            for i in 0..10 {
                let f = i as f32 / 10.;
                ui.fill_rect(Rect::new(sr.x, sr.y + sr.h * f, sr.w, sr.h / 10. + 0.002), Color::new(1.0, 0.58, 0.706, 0.06 * (1. - f)));
            }
            for k in 0..7 {
                let hx = sr.x + 0.12 + k as f32 * 0.3;
                ui.text("♡").pos(hx, sr.y + 0.06).anchor(0.5, 0.5).no_baseline().size(0.5).color(Color::new(1.0, 0.776, 0.847, 0.10)).draw();
            }
        });

        let gtop = cr.y + 0.155;
        let gh = cr.bottom() - gtop - 0.03;
        let cols = 3usize;
        let gap = 0.025_f32;
        let side = 0.04_f32;
        let cw = (cr.w - side * 2. - gap * (cols as f32 - 1.)) / cols as f32;
        let chh = 0.18_f32;

        s.render_fader(ui, |ui| {
            ui.text(format!("♡ {}", tl!("label")))
                .pos(cr.x + 0.05, cr.y + 0.075)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.74)
                .color(accent)
                .draw();

            if let Some(msgs) = &mut self.msgs {
                if msgs.is_empty() {
                    ui.text(tl!("no-msg")).pos(cr.center().x, gtop + gh * 0.4).anchor(0.5, 0.5).no_baseline().size(0.7).color(muted).draw();
                } else {
                    self.btns_scroll.size((cr.w, gh));
                    ui.scope(|ui| {
                        ui.dx(cr.x);
                        ui.dy(gtop);
                        self.btns_scroll.render(ui, |ui| {
                            let count = msgs.len();
                            for (index, item) in msgs.iter_mut().enumerate() {
                                let col = index % cols;
                                let row = index / cols;
                                let rr = Rect::new(side + col as f32 * (cw + gap), row as f32 * (chh + gap), cw, chh);
                                let chosen = Some(index) == self.index;
                                let time = item.0.time.with_timezone(&Local).format("%Y-%m-%d").to_string();
                                item.1.render_shadow(ui, rr, t, |ui, path| {
                                    ui.fill_path(&path, if chosen { card_sel } else { card_bg });
                                    ui.stroke_path(&path, 0.003, border_c);
                                    ui.fill_path(&Rect::new(rr.x + 0.018, rr.y + 0.022, 0.006, chh - 0.044).rounded(0.003), accent);
                                    ui.text(&item.0.title)
                                        .pos(rr.x + 0.036, rr.y + 0.03)
                                        .anchor(0., 0.)
                                        .no_baseline()
                                        .multiline()
                                        .max_width(rr.w - 0.05)
                                        .size(0.5)
                                        .color(dark_text)
                                        .draw();
                                    ui.text(format!("♡ {} · {}", item.0.author, time))
                                        .pos(rr.x + 0.036, rr.bottom() - 0.028)
                                        .anchor(0., 1.)
                                        .no_baseline()
                                        .max_width(rr.w - 0.05)
                                        .size(0.32)
                                        .color(muted)
                                        .draw();
                                });
                            }
                            let rows = count.div_ceil(cols);
                            (cr.w, rows as f32 * (chh + gap) + 0.02)
                        });
                    });
                }
            }
            if self.load_task.is_some() {
                ui.loading(cr.center().x, gtop + gh * 0.4, t, WHITE, ());
            }
        });

        if let Some(idx) = self.index {
            if self.msgs.as_ref().is_some_and(|m| idx < m.len()) {
                s.render_fader(ui, |ui| {
                    ui.fill_rect(ui.screen_rect(), semi_black(0.5));
                    let ov = Rect::new(cr.x + cr.w * 0.12, cr.y + 0.06, cr.w * 0.76, cr.h - 0.12);
                    ui.fill_path(&ov.rounded(rad), Color::new(0.20, 0.10, 0.155, 0.99));
                    ui.stroke_path(&ov.rounded(rad), 0.004, border_c);
                    let head_h = 0.085_f32;
                    ui.fill_path(&Rect::new(ov.x, ov.y, ov.w, head_h + rad).rounded(rad), accent);
                    let msg = &self.msgs.as_ref().unwrap()[idx].0;
                    ui.text(&msg.title)
                        .pos(ov.x + 0.03, ov.y + head_h * 0.5)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.55)
                        .max_width(ov.w - 0.13)
                        .color(WHITE)
                        .draw();
                    let cb = Rect::new(ov.right() - 0.065, ov.y + 0.016, 0.05, head_h - 0.032);
                    ui.text("✕").pos(cb.center().x, cb.center().y).anchor(0.5, 0.5).no_baseline().size(0.5).color(WHITE).draw();
                    self.close_btn.set(ui, cb);

                    let pad = 0.03_f32;
                    ui.text(tl!("subtitle", "author" => msg.author.as_str(), "time" => msg.time.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string()))
                        .pos(ov.x + pad, ov.y + head_h + 0.035)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.38)
                        .color(muted)
                        .draw();
                    ui.fill_rect(Rect::new(ov.x + pad, ov.y + head_h + 0.06, ov.w - pad * 2., 0.002), sep_c);
                    let csy = ov.y + head_h + 0.075;
                    let mw = ov.w - pad * 2.;
                    self.scroll.size((mw, ov.bottom() - csy - 0.02));
                    ui.scope(|ui| {
                        ui.dx(ov.x + pad);
                        ui.dy(csy);
                        self.scroll.render(ui, |ui| {
                            let r = ui.text(&msg.content).size(0.44).multiline().max_width(mw).color(dark_text).draw();
                            (mw, r.h + 0.04)
                        });
                    });
                });
            }
        }
        Ok(())
    }
}
