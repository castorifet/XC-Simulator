xcsim_core_l10n::tl_file!("login");

use crate::{
    client::{Client, LoginParams, User, UserManager, API_URL},
    get_data_mut,
    page::Fader,
    save_data,
};
use anyhow::Result;
use inputbox::{InputBox, InputMode};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use xcsim_core::{

    ext::{open_url, semi_black, RectExt},
    scene::{request_input, return_input, show_error, show_message, take_input},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Ui},
};
use regex::Regex;
use std::{borrow::Cow, future::Future};

static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\A[a-z0-9!#$%&'*+/=?^_'{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_'{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\z",
    )
    .unwrap()
});

fn validate_username(username: &str) -> Option<Cow<'static, str>> {
    if !(4..=12).contains(&username.chars().count()) {
        return Some(tl!("name-length-req"));
    }
    if username.chars().any(|it| it != '_' && it != '-' && !it.is_alphanumeric()) {
        return Some(tl!("name-has-illegal-char"));
    }
    None
}

fn draw_themed_input<'a>(
    btn: &mut DRectButton,
    ui: &mut Ui,
    r: Rect,
    t: f32,
    value: impl Into<Cow<'a, str>>,
    hint: impl Into<Cow<'a, str>>,
    size: f32,
) {
    let bg     = Color::new(0.22, 0.22, 0.22, 1.);
    let border = Color::new(0.42, 0.42, 0.42, 1.);
    let text_c = Color::new(0.984, 0.973, 0.886, 1.);
    let hint_c = Color::new(0.48, 0.48, 0.48, 1.);
    let value  = value.into();
    let hint   = hint.into();
    let empty  = value.trim().is_empty();
    btn.render_shadow(ui, r, t, |ui, _path| {
        let p = r.rounded(0.006);
        ui.fill_path(&p, bg);
        ui.stroke_path(&p, 0.003, border);
        let pad = 0.014_f32;
        let (txt, col) = if empty { (hint.as_ref(), hint_c) } else { (value.as_ref(), text_c) };
        ui.text(txt)
            .pos(r.x + pad, r.center().y)
            .anchor(0., 0.5).no_baseline()
            .size(size).max_width(r.w - pad * 2.)
            .color(col).draw();
    });
}

fn draw_primary_btn<'a>(btn: &mut DRectButton, ui: &mut Ui, r: Rect, t: f32, label: impl Into<Cow<'a, str>>, accent: Color) {
    let label = label.into();
    btn.render_shadow(ui, r, t, |ui, path| {
        ui.fill_path(&path, accent);
        ui.text(label.as_ref())
            .pos(r.center().x, r.center().y)
            .anchor(0.5, 0.5).no_baseline()
            .size(0.44).color(WHITE).draw();
    });
}

fn draw_secondary_btn<'a>(btn: &mut DRectButton, ui: &mut Ui, r: Rect, t: f32, label: impl Into<Cow<'a, str>>) {
    let label      = label.into();
    let bg         = Color::new(0.20, 0.20, 0.20, 1.);
    let border_col = Color::new(0.45, 0.45, 0.45, 1.);
    let text_c     = Color::new(0.984, 0.973, 0.886, 1.);
    btn.render_shadow(ui, r, t, |ui, _path| {
        let p = r.rounded(0.008);
        ui.fill_path(&p, bg);
        ui.stroke_path(&p, 0.005, border_col);
        ui.text(label.as_ref())
            .pos(r.center().x, r.center().y)
            .anchor(0.5, 0.5).no_baseline()
            .size(0.44).color(text_c).draw();
    });
}

pub struct Login {
    fader: Fader,
    show: bool,

    input_email:     DRectButton,
    input_pwd:       DRectButton,
    input_reg_email: DRectButton,
    input_reg_name:  DRectButton,
    input_reg_pwd:   DRectButton,

    btn_to_reg:   DRectButton,
    btn_to_login: DRectButton,
    btn_reg:      DRectButton,
    btn_login:    DRectButton,
    btn_forget_pwd: RectButton,

    t_email:     String,
    t_pwd:       String,
    t_reg_email: String,
    t_reg_name:  String,
    t_reg_pwd:   String,

    start_time: f32,
    in_reg: bool,

    task: Option<(&'static str, Task<Result<Option<User>>>)>,
}

impl Login {
    const TIME: f32 = 0.7;

    pub fn new() -> Self {
        Self {
            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            input_email:     DRectButton::new().with_delta(-0.002),
            input_pwd:       DRectButton::new().with_delta(-0.002),
            input_reg_email: DRectButton::new().with_delta(-0.002),
            input_reg_name:  DRectButton::new().with_delta(-0.002),
            input_reg_pwd:   DRectButton::new().with_delta(-0.002),

            btn_to_reg:    DRectButton::new(),
            btn_to_login:  DRectButton::new(),
            btn_reg:       DRectButton::new(),
            btn_login:     DRectButton::new(),
            btn_forget_pwd: RectButton::new(),

            t_email:     String::new(),
            t_pwd:       String::new(),
            t_reg_email: String::new(),
            t_reg_name:  String::new(),
            t_reg_pwd:   String::new(),

            start_time: f32::NAN,
            in_reg: false,

            task: None,
        }
    }

    #[inline]
    fn start(&mut self, desc: &'static str, future: impl Future<Output = Result<Option<User>>> + Send + 'static) {
        self.task = Some((desc, Task::new(future)));
    }

    pub fn enter(&mut self, t: f32) {
        self.fader.sub(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.show = false;
        self.fader.back(t);
    }

    fn register(&mut self) -> Option<Cow<'static, str>> {
        let email = self.t_reg_email.clone();
        let name  = self.t_reg_name.clone();
        let pwd   = self.t_reg_pwd.clone();
        if let Some(error) = validate_username(&name) {
            show_message(error).error();
        }
        if !EMAIL_REGEX.is_match(&email) {
            return Some(tl!("illegal-email"));
        }
        if !(8..=32).contains(&pwd.len()) {
            return Some(tl!("pwd-length-req"));
        }
        self.start("register", async move {
            Client::register(&email, &name, &pwd).await?;
            Ok(None)
        });
        None
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.fader.transiting() || self.task.is_some() || !self.start_time.is_nan() {
            return true;
        }
        if self.show {
            if !Ui::dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss(t);
                return true;
            }
            if self.input_email.touch(touch, t) {
                request_input("email", InputBox::new().default_text(&self.t_email));
                return true;
            }
            if self.input_pwd.touch(touch, t) {
                request_input("pwd", InputBox::new().default_text(&self.t_pwd).mode(InputMode::Password));
                return true;
            }
            if self.input_reg_email.touch(touch, t) {
                request_input("reg_email", InputBox::new().default_text(&self.t_reg_email));
                return true;
            }
            if self.input_reg_name.touch(touch, t) {
                request_input("reg_name", InputBox::new().default_text(&self.t_reg_name));
                return true;
            }
            if self.input_reg_pwd.touch(touch, t) {
                request_input("reg_pwd", InputBox::new().default_text(&self.t_reg_pwd).mode(InputMode::Password));
                return true;
            }
            if self.btn_to_reg.touch(touch, t) || self.btn_to_login.touch(touch, t) {
                self.start_time = t;
                return true;
            }
            if self.btn_reg.touch(touch, t) {
                if let Some(error) = self.register() {
                    show_message(error).error();
                }
                return true;
            }
            if self.btn_login.touch(touch, t) {
                let email = self.t_email.clone();
                let pwd   = self.t_pwd.clone();
                self.start("login", async move {
                    Client::login(LoginParams::Password { email: &email, password: &pwd }).await?;
                    Ok(Some(Client::get_me().await?))
                });
                return true;
            }
            if self.btn_forget_pwd.touch(touch) {
                button_hit();
                let _ = open_url(&format!("{API_URL}/reset-password"));
            }
            return true;
        }
        false
    }

    pub fn update(&mut self, t: f32) -> Result<()> {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
        if let Some((id, text)) = take_input() {
            'tmp: {
                let tmp = match id.as_str() {
                    "email"     => &mut self.t_email,
                    "pwd"       => &mut self.t_pwd,
                    "reg_email" => &mut self.t_reg_email,
                    "reg_name"  => &mut self.t_reg_name,
                    "reg_pwd"   => &mut self.t_reg_pwd,
                    _ => { return_input(id, text); break 'tmp; }
                };
                *tmp = text;
            }
        }
        if let Some((action, task)) = &mut self.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("action-failed", "action" => *action))),
                    Ok(user) => {
                        if let Some(user) = user {
                            UserManager::request(user.id);
                            get_data_mut().me = Some(user);
                            save_data()?;
                        }
                        self.t_pwd.clear();
                        show_message(tl!("action-success", "action" => *action)).ok();
                        if *action == "register" {
                            Dialog::simple(tl!("email-sent")).show();
                            self.t_reg_email.clear();
                            self.t_reg_name.clear();
                            self.t_reg_pwd.clear();
                            self.start_time = t;
                        }
                        if *action == "login" {
                            self.dismiss(t);
                        }
                    }
                }
                self.task = None;
            }
        }
        Ok(())
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        self.fader.reset();
        if !self.show && !self.fader.transiting() {
            if self.task.is_some() {
                ui.full_loading_simple(t);
            }
            return;
        }

        let accent     = crate::theme::FIREFLY_PINK_DEEP;
        let body_bg    = Color::new(0.165, 0.110, 0.180, 1.);
        let dark_text  = Color::new(0.984, 0.973, 0.886, 1.);
        let muted_text = Color::new(0.55, 0.55, 0.55, 1.);

        let p_fade = if self.show { 1. } else { -self.fader.progress(t) };
        ui.fill_rect(ui.screen_rect(), semi_black(p_fade * 0.7));

        self.fader.for_sub(|f| {
            f.render(ui, t, |ui| {
                let mut wr = Ui::dialog_rect();
                wr.y -= 0.03;
                wr.h += 0.06;

                ui.fill_path(&wr.feather(0.014).rounded(0.05), Color::new(0.949, 0.412, 0.580, 0.40));
                ui.fill_path(&wr.rounded(0.04), body_bg);

                let title_h = 0.10_f32;
                let title_r = Rect::new(wr.x, wr.y, wr.w, title_h);
                let title_label = if self.in_reg { tl!("register") } else { tl!("login") };
                ui.text(title_label)
                    .pos(title_r.x + 0.03, title_r.center().y)
                    .anchor(0., 0.5).no_baseline()
                    .size(0.54).color(dark_text)
                    .draw();
                ui.fill_path(&Rect::new(wr.x + 0.03, wr.y + title_h - 0.006, 0.14, 0.006).rounded(0.003), accent);

                let content_r = Rect::new(wr.x, wr.y + title_h, wr.w, wr.h - title_h);
                ui.scissor(content_r, |ui| {

                    let slide = (if self.start_time.is_nan() {
                        if self.in_reg { 0. } else { -1. }
                    } else {
                        let p = ((t - self.start_time) / Self::TIME).clamp(0., 1.);
                        let p = 1. - (1. - p).powi(3);
                        let res = if self.in_reg { -p } else { p - 1. };
                        if p >= 1. {
                            self.in_reg = !self.in_reg;
                            self.start_time = f32::NAN;
                        }
                        res
                    }) * wr.h;
                    ui.dy(slide);

                    let pad = 0.035_f32;
                    let btn_h = 0.085_f32;
                    let btn_pad = 0.045_f32;

                    {
                        let top = content_r.y;
                        ui.text(tl!("register"))
                            .pos(wr.x + 0.04, top + 0.04)
                            .anchor(0., 0.)
                            .size(0.72)
                            .color(dark_text)
                            .draw();

                        let input_top = top + 0.13;
                        let mut r = Rect::new(wr.x + pad, input_top, wr.w - pad * 2., 0.095);
                        draw_themed_input(&mut self.input_reg_email, ui, r, t, &self.t_reg_email, tl!("email"), 0.58);
                        r.y += r.h + 0.018;
                        draw_themed_input(&mut self.input_reg_name, ui, r, t, &self.t_reg_name, tl!("username"), 0.58);
                        r.y += r.h + 0.018;
                        draw_themed_input(&mut self.input_reg_pwd, ui, r, t, "*".repeat(self.t_reg_pwd.len()), tl!("password"), 0.58);

                        let by = content_r.y + wr.h - btn_h - btn_pad - title_h;
                        let half = (wr.w - pad * 2. - 0.014) / 2.;
                        let mut br = Rect::new(wr.x + pad, by, half, btn_h);
                        draw_secondary_btn(&mut self.btn_to_login, ui, br, t, tl!("back-login"));
                        br.x += half + 0.014;
                        draw_primary_btn(&mut self.btn_reg, ui, br, t, tl!("register"), accent);
                    }

                    ui.dy(wr.h);

                    {
                        let top = content_r.y;
                        ui.text(tl!("login"))
                            .pos(wr.x + 0.04, top + 0.04)
                            .anchor(0., 0.)
                            .size(0.72)
                            .color(dark_text)
                            .draw();
                        let sub_r = ui
                            .text(tl!("login-sub"))
                            .pos(wr.x + 0.046, top + 0.115)
                            .anchor(0., 0.)
                            .size(0.38)
                            .color(muted_text)
                            .max_width(wr.w - 0.05)
                            .multiline()
                            .draw();

                        let input_top = sub_r.bottom() + 0.04;
                        let mut r = Rect::new(wr.x + pad, input_top, wr.w - pad * 2., 0.095);
                        draw_themed_input(&mut self.input_email, ui, r, t, &self.t_email, tl!("email"), 0.58);
                        r.y += r.h + 0.018;
                        draw_themed_input(&mut self.input_pwd, ui, r, t, "*".repeat(self.t_pwd.len()), tl!("password"), 0.58);

                        let link_r = ui
                            .text(tl!("forget-password"))
                            .pos(wr.right() - pad, r.bottom() + 0.016)
                            .anchor(1., 0.)
                            .size(0.38)
                            .color(accent)
                            .draw();
                        self.btn_forget_pwd.set(ui, link_r.feather(0.016));

                        let by = content_r.y + wr.h - btn_h - btn_pad - title_h;
                        let half = (wr.w - pad * 2. - 0.014) / 2.;
                        let mut br = Rect::new(wr.x + pad, by, half, btn_h);
                        draw_secondary_btn(&mut self.btn_to_reg, ui, br, t, tl!("register"));
                        br.x += half + 0.014;
                        draw_primary_btn(&mut self.btn_login, ui, br, t, tl!("login"), accent);
                    }
                });
            });
        });

        if self.task.is_some() {
            ui.full_loading_simple(t);
        }
    }
}
