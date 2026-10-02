xcsim_core_l10n::tl_file!("home");

use super::{
    load_font_with_cksum, set_bold_font, LibraryPage, MessagePage, NextPage, Page, ResPackPage, SFader, SettingsPage, SharedState,
    BOLD_FONT_CKSUM,
};
use crate::{
    anim::Anim,
    client::{recv_raw, Character, Client, LoginParams, User, UserManager},
    dir, get_data, get_data_mut,
    icons::Icons,
    login::Login,
    save_data,
    scene::{check_read_tos_and_policy, ProfileScene, JUST_LOADED_TOS},
    sync_data,
    threed::ThreeD,
};
use ::rand::{random, thread_rng, Rng};
use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use image::DynamicImage;
use macroquad::prelude::*;
use xcsim_core::{
    ext::{open_url, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    scene::{show_error, NextScene},
    task::Task,
    ui::{button_hit_large, ClipType, DRectButton, Dialog, FontArc, RectButton, Scroll, Ui},
};
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    borrow::Cow,
    sync::{atomic::Ordering, Arc},
};
use tap::Tap;
use tracing::{info, warn};

const BOARD_SWITCH_TIME: f32 = 4.;
const BOARD_TRANSIT_TIME: f32 = 1.2;

type BoldFontUpdateTask = Task<Result<Option<(FontArc, String)>>>;

#[derive(Deserialize)]
struct Version {
    version: semver::Version,
    date: NaiveDate,
    description: String,
    url: String,
}

pub struct HomePage {
    icons: Arc<Icons>,

    btn_play: DRectButton,
    btn_respack: DRectButton,
    btn_msg: DRectButton,
    btn_settings: DRectButton,
    btn_user: DRectButton,

    btn_about: RectButton,
    btn_feedback: RectButton,
    btn_changelog: RectButton,
    btn_exit: RectButton,
    btn_more: RectButton,

    next_page: Option<NextPage>,

    login: Login,
    update_task: Option<Task<Result<User>>>,

    need_back: bool,
    sf: SFader,

    board_task: Option<Task<Result<Option<(DynamicImage, String, String, String)>>>>,
    board_last_time: f32,
    board_last: Option<String>,
    board_tex_last: Option<SafeTexture>,
    board_tex: Option<SafeTexture>,
    board_dir: bool,
    board_name: Option<String>,
    board_composer: Option<String>,
    board_level: Option<String>,

    has_new_task: Option<Task<Result<bool>>>,
    has_new: bool,

    check_update_task: Option<Task<Result<Option<Version>>>>,
    check_bold_font_update_task: Option<BoldFontUpdateTask>,

    btn_play_3d: ThreeD,
    btn_other_3d: ThreeD,

    character: Character,
    char_appear_p: Anim<f32>,
    char_last_illu: Option<String>,
    char_last_user_id: Option<i32>,
    char_fetch_task: Option<Task<Result<Character>>>,
    char_illu: Option<SafeTexture>,
    char_illu_task: Option<Task<Result<DynamicImage>>>,

    char_screen_p: Anim<f32>,
    char_btn: RectButton,
    char_text_start: f32,
    char_cached_size: f32,
    char_scroll: Scroll,
    char_edit_btn: RectButton,

    credits_scroll: Scroll,

    enter_anim: Anim<f32>,
    first_in: bool,

    #[cfg(feature = "aa")]
    beian_btn: RectButton,
}

impl HomePage {
    pub async fn new() -> Result<Self> {
        let update_task = if get_data().config.offline_mode {
            None
        } else if let Some(u) = &get_data().me {
            UserManager::request(u.id);
            Some(Task::new(async {
                Client::login(LoginParams::RefreshToken {
                    token: &get_data().tokens.as_ref().unwrap().1,
                })
                .await?;
                Client::get_me().await
            }))
        } else {
            None
        };

        let flavor = match load_file("flavor").await.map(String::from_utf8) {
            Ok(Ok(flavor)) => flavor.trim().to_owned(),
            _ => "none".to_owned(),
        };

        let mut res = Self {
            icons: Arc::new(Icons::new().await?),

            btn_play: DRectButton::new().with_delta(-0.01).no_sound(),
            btn_respack: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_msg: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_settings: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_user: DRectButton::new().with_delta(-0.003),

            btn_about: RectButton::new(),
            btn_feedback: RectButton::new(),
            btn_changelog: RectButton::new(),
            btn_exit: RectButton::new(),
            btn_more: RectButton::new(),

            next_page: None,

            login: Login::new(),
            update_task,

            need_back: false,
            sf: SFader::new(),

            board_task: None,
            board_last_time: f32::NEG_INFINITY,
            board_last: None,
            board_tex_last: None,
            board_tex: None,
            board_dir: false,
            board_name: None,
            board_composer: None,
            board_level: None,

            has_new_task: None,
            has_new: false,

            check_update_task: Some(Task::new(async move {
                Ok(recv_raw(Client::get("/check-update").query(&[("version", env!("CARGO_PKG_VERSION")), ("flavor", &flavor)]))
                    .await?
                    .json()
                    .await?)
            })),
            check_bold_font_update_task: {
                let cksum = BOLD_FONT_CKSUM.with(|it| it.borrow().clone());
                Some(Task::new(async move {
                    let resp = Client::get("/font-bold").query(&[("cksum", cksum)]).send().await?;
                    if resp.status() == StatusCode::NOT_MODIFIED {
                        info!("bold font not modified");
                        return Ok(None);
                    }
                    if !resp.status().is_success() {
                        let status = resp.status().as_str().to_owned();
                        let text = resp.text().await.context("failed to receive text")?;
                        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(detail) = what["error"].as_str() {
                                bail!("request failed ({status}): {detail}");
                            }
                        }
                        bail!("request failed ({status}): {text}");
                    }
                    info!("downloading new bold font");
                    let bytes = resp.bytes().await?;
                    std::fs::write(dir::bold_font_path()?, &bytes).context("failed to save font")?;
                    Ok(Some(load_font_with_cksum(bytes.to_vec())?))
                }))
            },

            btn_play_3d: ThreeD::new(),
            btn_other_3d: ThreeD::new().tap_mut(|it| {
                it.anchor = vec2(0.2, -0.2);
                it.angle = 0.14;
                it.sync();
            }),

            character: get_data().character.clone().unwrap_or_default(),
            char_appear_p: Anim::new(0.),
            char_last_illu: None,
            char_last_user_id: None,
            char_fetch_task: None,
            char_illu: None,
            char_illu_task: None,
            char_screen_p: Anim::new(0.),
            char_btn: RectButton::new(),
            char_text_start: 0.,
            char_cached_size: 0.,
            char_scroll: Scroll::new().use_clip(ClipType::Clip),
            char_edit_btn: RectButton::new(),

            credits_scroll: Scroll::new().use_clip(ClipType::Clip),

            enter_anim: Anim::new(1.),
            first_in: true,

            #[cfg(feature = "aa")]
            beian_btn: RectButton::new(),
        };
        res.load_char_illu();

        Ok(res)
    }
}

impl HomePage {
    fn load_char_illu(&mut self) {
        let key = if self.character.illust == "@" {
            format!("@{}", self.character.id)
        } else {
            self.character.illust.clone()
        };
        if self.char_last_illu.as_ref() == Some(&key) {
            return;
        }
        self.char_last_illu = Some(key);

        self.char_appear_p.set(0.);

        #[cfg(closed)]
        if self.character.illust == "@" {
            let id = self.character.id.clone();
            self.char_illu_task =
                Some(Task::new(
                    async move { Ok(image::load_from_memory(&crate::inner::resolve_data(load_file(&format!("res/{id}.char")).await?))?) },
                ));
        } else {
            let file = crate::page::File {
                url: self.character.illust.clone(),
            };
            self.char_illu_task =
                Some(Task::new(async move { Ok(image::load_from_memory(&crate::inner::resolve_data(file.fetch().await?.to_vec()))?) }));
        }
    }

    fn fetch_has_new(&mut self) {
        if get_data().config.offline_mode || get_data().me.is_none() || get_data().tokens.is_none() {
            self.has_new_task = None;
            self.has_new = false;
            return;
        }
        let time = get_data().message_check_time.unwrap_or_default();
        self.has_new_task = Some(Task::new(async move {
            #[derive(Deserialize)]
            struct Resp {
                has: bool,
            }
            let resp: Resp = recv_raw(Client::get("/message/has_new").query(&[("checked", time)]))
                .await?
                .json()
                .await?;
            Ok(resp.has)
        }));
    }

    fn render_not_char(&mut self, ui: &mut Ui, s: &mut SharedState) {
        let t = s.t;
        let top = ui.top;

        let icon_play = self.icons.play.clone();
        let icon_respack = self.icons.respack.clone();
        let icon_settings = self.icons.settings.clone();
        let icon_msg = self.icons.msg.clone();
        let has_new = self.has_new;

        let ep = self.enter_anim.now(t).clamp(0., 1.);
        let a = ep;
        let dx = (1.0 - ep) * -0.06;

        let white_a = Color::new(0.97, 0.98, 1.0, a);
        let sub_a = Color::new(0.82, 0.86, 0.92, 0.85 * a);

        let bx = -0.95 + dx;
        let bw = 0.45;
        let play_r = Rect::new(bx, -0.345, bw, 0.215);
        let respack_r = Rect::new(bx, -0.105, bw, 0.155);
        let settings_r = Rect::new(bx, 0.065, bw, 0.155);
        let messages_r = Rect::new(bx, 0.235, bw, 0.155);

        self.btn_play.config.radius = 0.024;
        self.btn_respack.config.radius = 0.02;
        self.btn_settings.config.radius = 0.02;
        self.btn_msg.config.radius = 0.02;

        s.render_fader(ui, |ui| draw_nav(ui, &mut self.btn_play, play_r, t, a, icon_play, &tl!("nav-play"), &tl!("nav-play-sub"), true, false));
        s.render_fader(ui, |ui| draw_nav(ui, &mut self.btn_respack, respack_r, t, a, icon_respack, &tl!("nav-respack"), &tl!("nav-respack-sub"), false, false));
        s.render_fader(ui, |ui| draw_nav(ui, &mut self.btn_settings, settings_r, t, a, icon_settings, &tl!("nav-settings"), &tl!("nav-settings-sub"), false, false));
        s.render_fader(ui, |ui| draw_nav(ui, &mut self.btn_msg, messages_r, t, a, icon_msg, &tl!("nav-messages"), &tl!("nav-messages-sub"), false, has_new));

        let card = Rect::new(-0.47, -top + 0.165, 0.84, 2. * top - 0.325);
        let rad = 0.03;
        let illu = self.char_illu.clone();
        let abstract_tex = self.icons.r#abstract.clone();
        let board_name = self.board_name.clone().unwrap_or_else(|| tl!("board-name").into_owned());
        let board_comp = self.board_composer.clone().unwrap_or_else(|| tl!("board-composer").into_owned());
        let board_level = self.board_level.clone().unwrap_or_else(|| tl!("board-level").into_owned());
        s.render_fader(ui, |ui| {
            ui.fill_path(&card.rounded(rad), Color::new(0.05, 0.06, 0.10, a));
            let texref = illu.as_ref().map(|it| **it).unwrap_or(*abstract_tex);
            ui.fill_path(&card.rounded(rad), (texref, card, ScaleType::CropCenter, white_a));

            let grad = Rect::new(card.x + rad, card.bottom() - 0.32, card.w - 2. * rad, 0.32);
            fill_vgrad(ui, grad, 0.82, (0.02, 0.03, 0.06));

            let name_max = card.w - 0.09;
            let mut nsz = 0.95;
            let nw = ui.text(&board_name).size(nsz).measure().w;
            if nw > name_max {
                nsz *= name_max / nw;
            }
            draw_soft_text(ui, &board_name, card.x + 0.045, card.bottom() - 0.075, (0., 1.), nsz, white_a);
            let comp_max = card.w - 0.10;
            let mut csz = 0.42;
            let cw = ui.text(&board_comp).size(csz).measure().w;
            if cw > comp_max {
                csz *= comp_max / cw;
            }
            draw_soft_text(ui, &board_comp, card.x + 0.05, card.bottom() - 0.04, (0., 1.), csz, sub_a);

            draw_soft_text(ui, &board_level, card.right() - 0.04, card.bottom() - 0.06, (1., 1.), 0.95, white_a);
        });

        let panel = Rect::new(0.385, card.y, 0.55, card.h);

        s.render_fader(ui, |ui| {
            ui.fill_path(&panel.rounded(0.014), Color::new(0.063, 0.078, 0.125, 0.50 * a));
        });

        s.render_fader(ui, |ui| {
            let mr = ui
                .text(tl!("credits-more"))
                .pos(panel.right() - 0.045, panel.y + 0.06)
                .anchor(1., 0.5)
                .no_baseline()
                .size(0.4)
                .color(sub_a)
                .draw();
            self.btn_more.set(ui, mr.feather(0.012));
        });

        let pad = 0.045;
        let content_top = 0.11;
        let sw = panel.w - pad * 2.;
        let sh = panel.h - content_top - 0.03;
        self.credits_scroll.size((sw, sh));
        s.render_fader(ui, |ui| {
            ui.scope(|ui| {
                ui.dx(panel.x + pad);
                ui.dy(panel.y + content_top);
                self.credits_scroll.render(ui, |ui| {
                    let mut h = 0.;

                    ui.text(tl!("credits-dev")).pos(0., h).no_baseline().size(0.52).color(white_a).draw();
                    h += 0.085;
                    let r = ui
                        .text(tl!("credits-dev-body"))
                        .pos(0., h)
                        .no_baseline()
                        .multiline()
                        .max_width(sw)
                        .size(0.4)
                        .color(sub_a)
                        .draw();
                    h += r.h + 0.04;

                    ui.fill_rect(Rect::new(0., h, sw, 0.0018), Color::new(1., 1., 1., 0.13 * a));
                    h += 0.05;

                    ui.text(tl!("credits-test")).pos(0., h).no_baseline().size(0.52).color(white_a).draw();
                    h += 0.085;
                    let r = ui
                        .text(tl!("credits-test-body"))
                        .pos(0., h)
                        .no_baseline()
                        .multiline()
                        .max_width(sw)
                        .size(0.4)
                        .color(sub_a)
                        .draw();
                    h += r.h + 0.02;
                    (sw, h)
                });
            });
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_nav(ui: &mut Ui, btn: &mut DRectButton, r: Rect, t: f32, a: f32, icon: SafeTexture, en: &str, zh: &str, highlight: bool, dot: bool) {
    let bg = if highlight {
        Color::new(0.176, 0.216, 0.314, 0.60 * a)
    } else {
        Color::new(0.106, 0.129, 0.196, 0.52 * a)
    };
    let white_a = Color::new(0.97, 0.98, 1.0, a);
    let sub_a = Color::new(0.82, 0.86, 0.92, 0.85 * a);
    btn.render_shadow(ui, r, t, |ui, path| {
        ui.fill_path(&path, bg);
        let isz = if highlight { 0.085 } else { 0.062 };
        let ir = Rect::new(r.x + 0.045, r.center().y - isz / 2., isz, isz);
        ui.fill_rect(ir, (*icon, ir, ScaleType::Fit, white_a));
        if dot {
            ui.fill_circle(ir.right() - 0.002, ir.y + 0.006, 0.009, RED);
        }
        let tx = r.x + 0.045 + isz + 0.05;
        let en_size = if highlight { 0.92 } else { 0.66 };
        draw_soft_text(ui, en, tx, r.center().y - 0.02, (0., 0.5), en_size, white_a);
        draw_soft_text(ui, zh, tx, r.center().y + 0.045, (0., 0.5), 0.4, sub_a);
    });
}

fn fill_vgrad(ui: &mut Ui, rect: Rect, bottom_alpha: f32, col: (f32, f32, f32)) {
    let n = 14;
    for i in 0..n {
        let f0 = i as f32 / n as f32;
        let f1 = (i + 1) as f32 / n as f32;
        let al = bottom_alpha * f1 * f1;
        ui.fill_rect(
            Rect::new(rect.x, rect.y + rect.h * f0, rect.w, rect.h * (f1 - f0) + 0.0015),
            Color::new(col.0, col.1, col.2, al),
        );
    }
}

fn draw_clock(ui: &mut Ui, cx: f32, cy: f32, r: f32, c: Color) {
    ui.stroke_circle(cx, cy, r, 0.006, c);
    ui.fill_rect(Rect::new(cx - 0.003, cy - r * 0.62, 0.006, r * 0.62), c);
    ui.fill_rect(Rect::new(cx - 0.002, cy - 0.003, r * 0.55, 0.006), c);
}

fn draw_soft_text(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), size: f32, color: Color) -> Rect {
    const OFFS: [(f32, f32); 8] = [
        (-0.004, 0.), (0.004, 0.), (0., -0.004), (0., 0.004),
        (-0.003, -0.003), (0.003, -0.003), (-0.003, 0.003), (0.003, 0.003),
    ];
    let halo = Color::new(0., 0., 0., 0.18 * color.a);
    for (dx, dy) in OFFS {
        ui.text(text)
            .pos(x + dx, y + dy)
            .anchor(anchor.0, anchor.1)
            .no_baseline()
            .size(size)
            .color(halo)
            .draw();
    }
    ui.text(text)
        .pos(x, y)
        .anchor(anchor.0, anchor.1)
        .no_baseline()
        .size(size)
        .color(color)
        .draw()
}

impl Page for HomePage {
    fn label(&self) -> Cow<'static, str> {
        "".into()
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.need_back {
            self.sf.enter(s.t);
            self.need_back = false;
        }
        self.enter_anim.start(0., 1., s.t, 0.6);
        self.fetch_has_new();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.sf.transiting() {
            return Ok(true);
        }
        let t = s.t;
        let rt = s.rt;
        if self.login.touch(touch, s.t) {
            return Ok(true);
        }
        if self.char_screen_p.now(rt) < 1e-2 {
            self.btn_play_3d.touch(touch, t);
            if self.btn_play.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(LibraryPage::new(Arc::clone(&self.icons), s.icons.clone())?)));
                return Ok(true);
            }
            if self.btn_respack.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(ResPackPage::new(Arc::clone(&self.icons))?)));
                return Ok(true);
            }
            if self.btn_msg.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(MessagePage::new())));
                return Ok(true);
            }
            if self.btn_settings.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(SettingsPage::new(self.icons.icon.clone(), self.icons.lang.clone()))));
                return Ok(true);
            }
        } else {
            return Ok(false);
        }
        if self.btn_user.touch(touch, t) {
            if let Some(me) = &get_data().me {
                self.need_back = true;
                self.sf.goto(t, ProfileScene::new(me.id, self.icons.user.clone(), s.icons.clone()));
            } else {
                self.login.enter(t);
            }
            return Ok(true);
        }
        if self.credits_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.btn_more.touch(touch) {
            Dialog::plain(tl!("staff-title"), tl!("staff-body")).show();
            return Ok(true);
        }
        if self.btn_about.touch(touch) {
            Dialog::plain(tl!("about-title"), tl!("about-body", "version" => env!("CARGO_PKG_VERSION"))).show();
            return Ok(true);
        }
        if self.btn_feedback.touch(touch) {
            Dialog::plain(tl!("feedback-title"), tl!("feedback-body")).show();
            return Ok(true);
        }
        if self.btn_changelog.touch(touch) {
            Dialog::plain(tl!("changelog-title"), tl!("changelog-body")).show();
            return Ok(true);
        }
        if self.btn_exit.touch(touch) {
            Dialog::plain(tl!("exit-title"), tl!("exit-body"))
                .buttons(vec![ttl!("cancel").into_owned(), tl!("exit-confirm").into_owned()])
                .listener(|_dialog, pos| {
                    if pos == 1 {
                        std::process::exit(0);
                    }
                    false
                })
                .show();
            return Ok(true);
        }
        #[cfg(feature = "aa")]
        if self.beian_btn.touch(touch) {
            let _ = open_url("https://beian.miit.gov.cn/#/home");
            return Ok(true);
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.login.update(t)?;
        let current_user = Some(get_data().me.as_ref().map_or(-1, |it| it.id));
        self.char_scroll.update(t);
        self.credits_scroll.update(t);
        if self.char_last_user_id != current_user {

self.char_fetch_task = None;
        }
        if let Some(task) = &mut self.update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {

                        if format!("{err:?}").contains("invalid token") {
                            get_data_mut().me = None;
                            get_data_mut().tokens = None;
                            let _ = save_data();
                            sync_data();
                        }

                        show_error(err.context(tl!("failed-to-update") + "\n" + tl!("note-try-login-again")));
                    }
                    Ok(val) => {
                        get_data_mut().me = Some(val);
                        save_data()?;
                    }
                }
                self.update_task = None;
            }
        }
        if self.board_task.is_none() && t - self.board_last_time > BOARD_SWITCH_TIME {
            let charts = &get_data().charts;
            let last_index = self
                .board_last
                .as_ref()
                .and_then(|path| charts.iter().position(|it| &it.local_path == path));
            if charts.is_empty() || (charts.len() == 1 && last_index.is_some()) {
                self.board_task = Some(Task::new(async move { Ok(None) }));
            } else {
                let mut index = thread_rng().gen_range(0..(charts.len() - last_index.is_some() as usize));
                if last_index.is_some_and(|it| it <= index) {
                    index += 1;
                }
                let path = charts[index].local_path.clone();
                let dir = xcsim_core::dir::Dir::new(format!("{}/{}", dir::charts()?, path))?;
                self.board_last = Some(path);
                self.board_task = Some(Task::new(async move {
                    let info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
                    let bytes = dir.read(&info.illustration)?;
                    Ok(Some((image::load_from_memory(&bytes)?, info.name, info.composer, info.level)))
                }));
            }
        }
        if let Some(task) = &mut self.board_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to load illustration for board");
                    }
                    Ok(image) => {
                        if let Some((image, name, composer, level)) = image {
                            let tex: SafeTexture = image.into();
                            self.board_tex_last = self.board_tex.replace(tex);
                            self.board_dir = random();
                            self.board_name = Some(name);
                            self.board_composer = Some(composer);
                            self.board_level = Some(level);
                        }
                    }
                }
                self.board_last_time = t;
                self.board_task = None;
            }
        }
        if let Some(task) = &mut self.has_new_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to load has new {:?}", err);
                    }
                    Ok(has) => {
                        self.has_new = has;
                    }
                }
                self.has_new_task = None;
            }
        }
        if let Some(task) = &mut self.check_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check update {:?}", err);
                    }
                    Ok(Some(ver)) => {
                        if get_data().ignored_version.as_ref().is_none_or(|it| it < &ver.version) {
                            Dialog::plain(
                                tl!("update", "version" => ver.version.to_string()),
                                tl!("update-desc", "date" => ver.date.to_string(), "desc" => ver.description),
                            )
                            .buttons(vec![
                                ttl!("cancel").into_owned(),
                                tl!("update-ignore").into_owned(),
                                tl!("update-go").into_owned(),
                            ])
                            .listener(move |_dialog, pos| {
                                match pos {
                                    1 => {
                                        get_data_mut().ignored_version = Some(ver.version.clone());
                                        let _ = save_data();
                                    }
                                    2 => {
                                        let _ = open_url(&ver.url);
                                    }
                                    _ => {}
                                }
                                false
                            })
                            .show();
                        }
                    }
                    _ => {}
                }
                self.check_update_task = None;
            }
        }
        if let Some(task) = &mut self.check_bold_font_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check bold font update {:?}", err);
                    }
                    Ok(None) => {}
                    Ok(Some(parsed)) => {
                        info!(cksum = parsed.1, "new bold font");
                        set_bold_font(parsed);
                    }
                }
                self.check_bold_font_update_task = None;
            }
        }
        if let Some(task) = &mut self.char_illu_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char illu");
                    }
                    Ok(image) => {
                        self.char_appear_p.goto(1., t, 0.5);
                        let tex: SafeTexture = image.into();
                        self.char_illu = Some(tex.with_mipmap());
                    }
                }
                self.char_illu_task = None;
            }
        }
        if let Some(task) = &mut self.char_fetch_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char");
                    }
                    Ok(char) => {
                        info!(?char, "char loaded");
                        self.character = char;
                        get_data_mut().character = Some(self.character.clone());
                        let _ = save_data();
                        self.char_cached_size = 0.;
                        self.load_char_illu();
                    }
                }
                self.char_fetch_task = None;
            }
        }
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(true, true);
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;

        if self.first_in {
            self.first_in = false;
            self.enter_anim.start(0., 1., t, 0.6);
        }

        let _ = rt;

        s.render_fader(ui, |ui| {
            let sr = ui.screen_rect();
            let bg = self
                .board_tex
                .as_ref()
                .or(self.char_illu.as_ref())
                .map(|it| **it)
                .unwrap_or(*self.icons.r#abstract);
            ui.fill_rect(sr, (bg, sr, ScaleType::CropCenter, WHITE));
            ui.fill_rect(sr, Color::new(0.05, 0.06, 0.11, 0.55));
        });

        self.render_not_char(ui, s);

        s.render_fader(ui, |ui| {
            let top = ui.top;
            draw_soft_text(ui, "XC-SIM", -0.05, top - 0.135, (0.5, 0.5), 1.15, crate::theme::FIREFLY_PINK);
        });

        s.fader.roll_back();

        s.render_fader(ui, |ui| {
            let top = ui.top;

            let rad = 0.05_f32;
            let ct = (-0.90, -top + 0.085);
            self.btn_user.config.radius = rad;
            let r = Rect::new(ct.0, ct.1, 0., 0.).feather(rad);
            self.btn_user.build(ui, t, r, |ui, _| {
                ui.avatar(
                    ct.0,
                    ct.1,
                    r.w / 2.,
                    t,
                    get_data()
                        .me
                        .as_ref()
                        .map(|user| UserManager::opt_avatar(user.id, &self.icons.user))
                        .unwrap_or(Err(self.icons.user.clone())),
                );
            });

            let info_x = ct.0 + rad + 0.025;
            let uid;
            if let Some(me) = &get_data().me {
                let name = me.name.clone();
                let rks = me.rks;
                uid = me.id;
                draw_soft_text(ui, &name, info_x, ct.1 - 0.022, (0., 0.5), 0.5, crate::theme::title_text());
                draw_soft_text(ui, &tl!("rks", "rks" => format!("{rks:.2}")), info_x, ct.1 + 0.016, (0., 0.5), 0.42, crate::theme::FIREFLY_PINK_SOFT);
            } else {
                uid = 0;
                draw_soft_text(ui, &tl!("not-logged-in"), info_x, ct.1 - 0.022, (0., 0.5), 0.5, crate::theme::cream_text(0.95));
                draw_soft_text(ui, &tl!("rks", "rks" => "0.00"), info_x, ct.1 + 0.016, (0., 0.5), 0.42, crate::theme::FIREFLY_PINK_SOFT);
            }

            let badge = Rect::new(info_x, ct.1 + 0.038, 0.08, 0.028);
            ui.fill_path(&badge.rounded(0.007), Color::new(1., 1., 1., 0.16));
            ui.text(tl!("uid", "id" => uid))
                .pos(badge.center().x, badge.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.3)
                .color(crate::theme::cream_text(0.8))
                .draw();

            draw_soft_text(ui, &tl!("title"), 0.0, -top + 0.085, (0.5, 0.5), 1.05, crate::theme::title_text());

            #[cfg(feature = "aa")]
            {
                let r = ui.screen_rect();
                let r = ui
                    .text("备案号：闽ICP备18008307号-64A")
                    .pos(r.x + 0.02, r.bottom() - 0.03)
                    .size(0.5)
                    .anchor(0., 1.)
                    .draw();
                self.beian_btn.set(ui, r);
            }
        });

        let icon_info = self.icons.info.clone();
        let icon_fb = self.icons.msg.clone();
        s.render_fader(ui, |ui| {
            let top = ui.top;
            let by = top - 0.05;
            let isz = 0.045_f32;
            let txt_c = crate::theme::cream_text(0.95);

            let x = -0.94;
            let ir = Rect::new(x, by - isz / 2., isz, isz);
            ui.fill_rect(ir, (*icon_info, ir, ScaleType::Fit, txt_c));
            let r1 = draw_soft_text(ui, &tl!("bottom-about"), x + isz + 0.02, by, (0., 0.5), 0.55, txt_c);
            self.btn_about.set(ui, Rect::new(x, by - 0.035, r1.right() - x, 0.07));

            let x = -0.70;
            let ir = Rect::new(x, by - isz / 2., isz, isz);
            ui.fill_rect(ir, (*icon_fb, ir, ScaleType::Fit, txt_c));
            let r2 = draw_soft_text(ui, &tl!("bottom-feedback"), x + isz + 0.02, by, (0., 0.5), 0.55, txt_c);
            self.btn_feedback.set(ui, Rect::new(x, by - 0.035, r2.right() - x, 0.07));

            let x = -0.42;
            draw_clock(ui, x + isz / 2., by, isz / 2., txt_c);
            let r3 = draw_soft_text(ui, &tl!("bottom-changelog"), x + isz + 0.02, by, (0., 0.5), 0.55, txt_c);
            self.btn_changelog.set(ui, Rect::new(x, by - 0.035, r3.right() - x, 0.07));

            let er = draw_soft_text(ui, &tl!("bottom-exit"), 0.95, by, (1., 0.5), 0.62, txt_c);
            self.btn_exit.set(ui, er.feather(0.02));
        });

        self.login.render(ui, t);
        self.sf.render(ui, t);

        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, s: &mut SharedState) -> NextScene {
        self.sf.next_scene(s.t).unwrap_or_default()
    }
}
