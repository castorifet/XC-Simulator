xcsim_core_l10n::tl_file!("settings");

use super::{NextPage, OffsetPage, Page, SharedState};
use crate::{
    dir, get_data, get_data_mut,
    popup::ChooseButton,
    save_data,
    scene::BGM_VOLUME_UPDATED,
    sync_data,
    tabs::{Tabs, TitleFn},
};
use anyhow::Result;
use bytesize::ByteSize;
use inputbox::InputBox;
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use xcsim_core::{
    ext::{open_url, poll_future, semi_white, LocalTask, RectExt, SafeTexture},
    scene::{request_input, return_input, show_error, show_message, take_input},
    task::Task,
    ui::{DRectButton, RectButton, Scroll, Slider, Ui, PREFER_REDUCED_MOTION},
};
use xcsim_core_l10n::{LanguageIdentifier, LANG_IDENTS, LANG_NAMES};
use reqwest::Url;
use serde::Deserialize;
use std::{borrow::Cow, fs, io, net::ToSocketAddrs, path::PathBuf, sync::atomic::Ordering};

fn save_unlock() {
    if let Ok(root) = crate::dir::root() {
        if let Err(e) = crate::unlock::save(&root) {
            xcsim_core::scene::show_error(anyhow::anyhow!("Failed to save unlock state: {e:#}"));
        }
    }
}

const ITEM_HEIGHT: f32 = 0.15;
const INTERACT_WIDTH: f32 = 0.26;
const STATUS_PAGE: &str = "https://status.phira.cn";

struct NameList(String);
impl<'de> Deserialize<'de> for NameList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = Vec::<String>::deserialize(deserializer)?;
        Ok(Self(s.join(", ")))
    }
}

#[derive(Deserialize)]
struct LocalizationListRaw {
    #[serde(rename = "en-US")]
    en_us: NameList,
    #[serde(rename = "fr-FR")]
    fr_fr: NameList,
    #[serde(rename = "de-DE")]
    de_de: NameList,
    #[serde(rename = "id-ID")]
    id_id: NameList,
    #[serde(rename = "ja-JP")]
    ja_jp: NameList,
    #[serde(rename = "ko-KR")]
    ko_kr: NameList,
    #[serde(rename = "pl-PL")]
    pl_pl: NameList,
    #[serde(rename = "pt-BR")]
    pt_br: NameList,
    #[serde(rename = "ru-RU")]
    ru_ru: NameList,
    #[serde(rename = "th-TH")]
    th_th: NameList,
    #[serde(rename = "zh-TW")]
    zh_tw: NameList,
    #[serde(rename = "tr-TR")]
    tr_tr: NameList,
    #[serde(rename = "vi-VN")]
    vi_vn: NameList,
}

struct LocalizationList(String);
impl<'de> Deserialize<'de> for LocalizationList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = LocalizationListRaw::deserialize(deserializer)?;
        Ok(Self(format!(
            "\
English (en-US)\n{}\n
French (fr-FR)\n{}\n
German (de-DE)\n{}\n
Indonesian (id-ID)\n{}\n
Japanese (ja-JP)\n{}\n
Korean (ko-KR)\n{}\n
Polish (pl-PL)\n{}\n
Portuguese (pt-BR)\n{}\n
Russian (ru-RU)\n{}\n
Thai (th-TH)\n{}\n
Traditional Chinese (zh-TW)\n{}\n
Turkish (tr-TR)\n{}\n
Vietnamese (vi-VN)\n{}",
            raw.en_us.0,
            raw.fr_fr.0,
            raw.de_de.0,
            raw.id_id.0,
            raw.ja_jp.0,
            raw.ko_kr.0,
            raw.pl_pl.0,
            raw.pt_br.0,
            raw.ru_ru.0,
            raw.th_th.0,
            raw.zh_tw.0,
            raw.tr_tr.0,
            raw.vi_vn.0
        )))
    }
}

#[derive(Deserialize)]
struct StaffList {
    development: NameList,
    operations: NameList,
    documentation: NameList,
    art: NameList,
    music: NameList,
    audio: NameList,
    community: NameList,
    localization: LocalizationList,
}

static STAFF_LIST: Lazy<StaffList> = Lazy::new(|| {
    let data = include_str!("../../staff.yml");
    serde_yaml::from_str(data).unwrap()
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingListType {
    General,
    Audio,
    Chart,
    Debug,
    Custom,
    About,
}

pub struct SettingsPage {
    list_general: GeneralList,
    list_audio: AudioList,
    list_chart: ChartList,
    list_debug: DebugList,
    list_custom: CustomList,

    tabs: Tabs<SettingListType>,
    scroll: Scroll,
    save_time: f32,

    icon: SafeTexture,

    xhus2_nav: [DRectButton; 6],

    about_title_btn: RectButton,
    about_icon_btn: RectButton,
    about_title_taps: u8,
    about_title_last: f64,
}

impl SettingsPage {
    const SAVE_TIME: f32 = 0.5;

    pub fn new(icon: SafeTexture, icon_lang: SafeTexture) -> Self {
        Self {
            list_general: GeneralList::new(icon_lang),
            list_audio: AudioList::new(),
            list_chart: ChartList::new(),
            list_debug: DebugList::new(),
            list_custom: CustomList::new(),

            tabs: Tabs::new([
                (SettingListType::General, || tl!("general")),
                (SettingListType::Audio, || tl!("audio")),
                (SettingListType::Chart, || tl!("chart")),
                (SettingListType::Debug, || tl!("debug")),
                (SettingListType::Custom, || tl!("ui")),
                (SettingListType::About, || tl!("about")),
            ] as [(SettingListType, TitleFn); 6]),

            scroll: Scroll::new(),
            save_time: f32::INFINITY,

            icon,

            xhus2_nav: std::array::from_fn(|_| DRectButton::new()),

            about_title_btn: RectButton::new(),
            about_icon_btn: RectButton::new(),
            about_title_taps: 0,
            about_title_last: f64::NEG_INFINITY,
        }
    }

    fn check_about_title_tap(&mut self, touch: &Touch, t: f64) -> bool {
        let hit = self.about_title_btn.touch(touch) || self.about_icon_btn.touch(touch);
        if hit {
            if t - self.about_title_last > 2.0 {
                self.about_title_taps = 0;
            }
            self.about_title_last = t;
            self.about_title_taps += 1;
            if self.about_title_taps >= 6 {
                self.about_title_taps = 0;
                request_input(
                    "_unlock_code",
                    InputBox::new().title("XC-SIM Unlock").prompt("Enter feature code:"),
                );
            }
            return true;
        }
        false
    }
}

const XHUS_ACCENT: Color = crate::theme::FIREFLY_PINK_DEEP;

impl SettingsPage {
    fn touch_xhus2(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        let rt = s.rt;

        for i in 0..6 {
            if self.xhus2_nav[i].touch(touch, rt) {
                self.tabs.goto(rt, i);
                self.scroll.y_scroller.halt();
                return Ok(false);
            }
        }

        if match self.tabs.selected() {
            SettingListType::General => self.list_general.top_touch(touch, t),
            SettingListType::Audio => self.list_audio.top_touch(touch, t),
            SettingListType::Chart => self.list_chart.top_touch(touch, t),
            SettingListType::Debug => self.list_debug.top_touch(touch, t),
            SettingListType::Custom => self.list_custom.top_touch(touch, t),
            SettingListType::About => false,
        } {
            return Ok(true);
        }

        if self.scroll.touch(touch, t) {
            return Ok(true);
        }

        if let Some(p) = match self.tabs.selected() {
            SettingListType::General => self.list_general.touch(touch, t)?,
            SettingListType::Audio => self.list_audio.touch(touch, t)?,
            SettingListType::Chart => self.list_chart.touch(touch, t)?,
            SettingListType::Debug => self.list_debug.touch(touch, t)?,
            SettingListType::Custom => self.list_custom.touch(touch, t)?,
            SettingListType::About => None,
        } {
            if p { self.save_time = t; }
            self.scroll.y_scroller.halt();
            return Ok(true);
        }
        if *self.tabs.selected() == SettingListType::About {
            if self.check_about_title_tap(touch, s.t as f64) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render_xhus2(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;
        let top = ui.top;
        let bar_y = -top;
        let total_h = top * 2.;

        const HDR_H: f32 = 0.155;
        const MARGIN: f32 = 0.045;
        const SECTION_HDR_H: f32 = 0.072;
        let full_x = -1.0 + MARGIN;
        let full_w = (1.0 - MARGIN) - full_x;
        let tab_h = 0.10_f32;
        let tab_gap = 0.014_f32;
        let content_x = full_x;
        let content_w = full_w;

        let c_bg         = Color::new(0.137, 0.094, 0.149, 1.);
        let c_nav_bg     = Color::new(0.110, 0.071, 0.125, 1.);
        let c_hdr_bg     = Color::new(0.094, 0.055, 0.106, 1.);
        let c_sec_hdr    = Color::new(0.176, 0.118, 0.188, 1.);
        let c_sep        = Color::new(1., 0.776, 0.847, 0.10);
        let c_accent     = crate::theme::FIREFLY_PINK;
        let c_cream      = crate::theme::FIREFLY_CREAM_SOFT;
        let _ = XHUS_ACCENT;

        let section_name: std::borrow::Cow<'static, str> = match self.tabs.selected() {
            SettingListType::General => tl!("general"),
            SettingListType::Audio   => tl!("audio"),
            SettingListType::Chart   => tl!("chart"),
            SettingListType::Debug   => tl!("debug"),
            SettingListType::Custom  => tl!("ui"),
            SettingListType::About   => tl!("about"),
        };
        let section_str = section_name.as_ref();

        s.fader.render(ui, t, |ui| {

            ui.fill_rect(Rect::new(-1., bar_y, 2., total_h), c_hdr_bg);

            let br = ui.back_rect();
            ui.fill_path(&br.feather(-0.004).rounded(0.02), Color::new(1.0, 0.58, 0.706, 0.14));
            ui.text("←")
                .pos(br.center().x, br.center().y)
                .anchor(0.5, 0.5).no_baseline().size(0.5)
                .color(c_accent).draw();
            let title_x = br.right() + 0.04;
            let title_y = bar_y + HDR_H * 0.38;
            let st_r = ui.text("Settings")
                .pos(title_x, title_y)
                .anchor(0., 0.5).no_baseline().size(0.9)
                .color(c_cream).draw();
            ui.text("♥")
                .pos(st_r.right() + 0.028, st_r.center().y)
                .anchor(0., 0.5).no_baseline().size(0.5)
                .color(c_accent).draw();
            ui.text(section_str)
                .pos(title_x + 0.002, title_y + 0.058)
                .anchor(0., 0.5).no_baseline().size(0.40)
                .color(Color::new(1.0, 0.776, 0.847, 0.8)).draw();
            ui.fill_path(&Rect::new(title_x, bar_y + HDR_H - 0.014, 0.13, 0.006).rounded(0.003), c_accent);

            let tab_y = bar_y + HDR_H;
            let card_top = tab_y + tab_h + 0.020;
            let card_h = (top - MARGIN) - card_top;

            let sel = self.tabs.selected_idx();
            let n = self.tabs.len();
            let pill_w = (full_w - tab_gap * (n as f32 - 1.)) / n as f32;
            for i in 0..n {
                let tx = full_x + i as f32 * (pill_w + tab_gap);
                let tr = Rect::new(tx, tab_y, pill_w, tab_h);
                let active = i == sel;
                if active {
                    ui.fill_path(&tr.feather(0.006).rounded(tab_h * 0.5), Color::new(1.0, 0.58, 0.706, 0.25));
                    ui.fill_path(&tr.rounded(tab_h * 0.5), c_accent);
                } else {
                    ui.fill_path(&tr.rounded(tab_h * 0.5), c_nav_bg);
                    ui.stroke_path(&tr.rounded(tab_h * 0.5), 0.0025, Color::new(1.0, 0.776, 0.847, 0.18));
                }
                ui.text(format!("{} {}", if active { "♥" } else { "♡" }, self.tabs.title(i)))
                    .pos(tr.center().x, tr.center().y)
                    .anchor(0.5, 0.5).no_baseline().size(0.42)
                    .max_width(pill_w - 0.025)
                    .color(if active { WHITE } else { semi_white(0.7) })
                    .draw();
                self.xhus2_nav[i].render_shadow(ui, tr, rt, |_, _| {});
            }

            let content_card = Rect::new(content_x, card_top, content_w, card_h);
            ui.fill_path(&content_card.feather(0.008).rounded(0.035), Color::new(1.0, 0.58, 0.706, 0.10));
            ui.fill_path(&content_card.rounded(0.03), c_bg);

            let sh_r = Rect::new(content_x, card_top, content_w, SECTION_HDR_H);
            ui.fill_path(&Rect::new(content_x + 0.018, card_top + 0.014, content_w - 0.036, SECTION_HDR_H - 0.018).rounded(0.018), c_sec_hdr);
            ui.text(section_str)
                .pos(content_x + 0.04, sh_r.center().y)
                .anchor(0., 0.5).no_baseline().size(0.5)
                .color(c_cream).draw();
            ui.fill_path(&Rect::new(content_x + 0.03, sh_r.bottom() + 0.002, content_w - 0.06, 0.003).rounded(0.0015), c_sep);

            let list_y = card_top + SECTION_HDR_H + 0.008;
            let list_h = card_h - SECTION_HDR_H - 0.02;
            let list_r = Rect::new(content_x + 0.012, list_y, content_w - 0.024, list_h);

            self.scroll.size((content_w - 0.08, list_h));
            ui.scissor(list_r, |ui| {
                ui.scope(|ui| {
                    ui.dx(content_x + 0.04);
                    ui.dy(list_y);
                    let render_r = Rect::new(0., 0., content_w - 0.08, list_h);
                    self.scroll.render(ui, |ui| match self.tabs.selected() {
                        SettingListType::General => self.list_general.render(ui, render_r, t),
                        SettingListType::Audio   => self.list_audio.render(ui, render_r, t),
                        SettingListType::Chart   => self.list_chart.render(ui, render_r, t),
                        SettingListType::Debug   => self.list_debug.render(ui, render_r, t),
                        SettingListType::Custom  => self.list_custom.render(ui, render_r, t),
                        SettingListType::About   => render_about(ui, render_r, &self.icon, &mut self.about_title_btn, &mut self.about_icon_btn),
                    });
                });
            });

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}

impl Page for SettingsPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn custom_title(&self) -> bool {
        true
    }

    fn exit(&mut self) -> Result<()> {
        BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
        if self.save_time.is_finite() {
            save_data()?;
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        self.touch_xhus2(touch, s)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let changed = match self.tabs.selected() {
            SettingListType::General => self.list_general.update(t)?,
            SettingListType::Audio => self.list_audio.update(t)?,
            SettingListType::Chart => self.list_chart.update(t)?,
            SettingListType::Debug => self.list_debug.update(t)?,
            SettingListType::Custom => self.list_custom.update(t)?,
            SettingListType::About => false,
        };
        self.scroll.update(t);
        if changed {
            self.save_time = t;
        }
        if t > self.save_time + Self::SAVE_TIME {
            save_data()?;
            self.save_time = f32::INFINITY;
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        self.render_xhus2(ui, s)
    }

    fn next_page(&mut self) -> NextPage {
        if matches!(self.tabs.selected(), SettingListType::Audio) {
            return self.list_audio.next_page().unwrap_or_default();
        }
        NextPage::None
    }
}

fn render_about(ui: &mut Ui, mut r: Rect, icon: &SafeTexture, title_btn: &mut RectButton, icon_btn: &mut RectButton) -> (f32, f32) {
    r.x = 0.;
    r.y = 0.;
    let ow = r.w;
    let r = r.feather(-0.02);

    let ct = r.center();
    let s = 0.1;
    let ir = Rect::new(ct.x - s, r.y + 0.05, s * 2., s * 2.);
    ui.fill_path(&ir.rounded(0.02), (**icon, ir));
    icon_btn.set(ui, ir);

    let staff = &*STAFF_LIST;
    let text = tl!(
        "about-content",
        "version" => format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH")),
        "development" => &staff.development.0,
        "operations" => &staff.operations.0,
        "documentation" => &staff.documentation.0,
        "art" => &staff.art.0,
        "music" => &staff.music.0,
        "audio" => &staff.audio.0,
        "community" => &staff.community.0,
        "localization" => &staff.localization.0
    );
    let (first, text) = text.split_once('\n').unwrap();
    let tr = ui
        .text(first)
        .pos(ct.x, ir.bottom() + 0.03)
        .anchor(0.5, 0.)
        .size(0.6)
        .draw();
    title_btn.set(ui, tr);

    let r = ui
        .text(text.trim())
        .pos(r.x, tr.bottom() + 0.06)
        .size(0.55)
        .multiline()
        .max_width(r.w)
        .h_center()
        .draw();

    (ow, r.bottom() + 0.03)
}

fn render_title<'a>(ui: &mut Ui, title: impl Into<Cow<'a, str>>, subtitle: Option<Cow<'a, str>>) -> f32 {
    const T_SZ: f32 = 0.49;
    const S_SZ: f32 = 0.335;
    const LEFT: f32 = 0.03;
    const PAD: f32 = 0.007;
    const S_MAX: f32 = 1.1;
    ui.fill_rect(
        Rect::new(-0.1, ITEM_HEIGHT - 0.001, 3.0, 0.001),
        Color::new(1., 0.776, 0.847, 0.09),
    );
    if let Some(subtitle) = subtitle {
        let title = title.into();
        let r1 = ui.text(Cow::clone(&title)).size(T_SZ).no_baseline().measure();
        let r2 = ui.text(Cow::clone(&subtitle)).size(S_SZ).max_width(S_MAX).no_baseline().measure();
        let h = r1.h + PAD + r2.h;
        ui.text(Cow::clone(&subtitle))
            .pos(LEFT, (ITEM_HEIGHT + h) / 2.)
            .anchor(0., 1.)
            .size(S_SZ)
            .max_width(S_MAX)
            .color(semi_white(0.48))
            .draw();
        let r = ui
            .text(title)
            .pos(LEFT, (ITEM_HEIGHT - h) / 2.)
            .no_baseline()
            .size(T_SZ)
            .color(crate::theme::FIREFLY_CREAM_SOFT)
            .draw();
        r.right()
    } else {
        ui.text(title.into())
            .pos(LEFT, ITEM_HEIGHT / 2.)
            .anchor(0., 0.5)
            .no_baseline()
            .size(T_SZ)
            .color(crate::theme::FIREFLY_CREAM_SOFT)
            .draw()
            .right()
    }
}

#[inline]
fn render_switch(ui: &mut Ui, r: Rect, t: f32, btn: &mut DRectButton, on: bool) {

    let accent = crate::theme::FIREFLY_PINK_DEEP;
    let border = Color::new(1., 0.776, 0.847, 0.55);
    let glyph = crate::theme::FIREFLY_CREAM_SOFT;
    btn.build(ui, t, r, |ui, _path| {

        let sz = (r.h * 0.46).min(0.075);
        let bx = r.right() - sz - 0.015;
        let by = r.center().y - sz / 2.;
        let rad = sz * 0.28;
        let brect = Rect::new(bx, by, sz, sz);

        if on {

            ui.fill_path(&brect.feather(0.004).rounded(rad + 0.004), Color::new(1., 0.58, 0.706, 0.35));
            ui.fill_path(&brect.rounded(rad), accent);
            ui.text("✓")
                .pos(bx + sz / 2., by + sz / 2. + sz * 0.04)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(sz * 12.)
                .color(glyph)
                .draw();
        } else {

            ui.fill_path(&brect.rounded(rad), border);
            ui.fill_path(&brect.feather(-0.0032).rounded(rad - 0.003), Color::new(0.137, 0.094, 0.149, 1.));
        }
    });
}

#[inline]
fn render_textfield<'a>(ui: &mut Ui, r: Rect, t: f32, btn: &mut DRectButton, text: impl Into<Cow<'a, str>>, size: f32) {

    let fill = Color::new(0.165, 0.110, 0.180, 0.95);
    let border = Color::new(1., 0.776, 0.847, 0.5);
    let text_c = Color::new(0.984, 0.973, 0.886, 1.);
    let bw = 0.0028;
    btn.build(ui, t, r, |ui, _path| {
        ui.fill_rect(r, fill);

        ui.fill_rect(Rect::new(r.x, r.y, r.w, bw), border);
        ui.fill_rect(Rect::new(r.x, r.bottom() - bw, r.w, bw), border);
        ui.fill_rect(Rect::new(r.x, r.y, bw, r.h), border);
        ui.fill_rect(Rect::new(r.right() - bw, r.y, bw, r.h), border);
        ui.text(text)
            .pos(r.x + 0.014, r.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(size)
            .max_width(r.w - 0.028)
            .color(text_c)
            .draw();
    });
}

#[inline]
fn right_rect(w: f32) -> Rect {
    let rh = ITEM_HEIGHT * 2. / 3.;
    Rect::new(w - 0.3, (ITEM_HEIGHT - rh) / 2., INTERACT_WIDTH, rh)
}

struct GeneralList {
    icon_lang: SafeTexture,

    lang_btn: ChooseButton,

    #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
    fullscreen_btn: DRectButton,

    cache_btn: DRectButton,
    offline_btn: DRectButton,
    server_status_btn: DRectButton,
    mp_btn: DRectButton,
    mp_addr_btn: DRectButton,
    #[cfg(not(target_env = "ohos"))]
    lowq_btn: DRectButton,
    prefer_reduced_motion_btn: DRectButton,
    insecure_btn: DRectButton,
    enable_anys_btn: DRectButton,
    anys_gateway_btn: DRectButton,

    cache_size: Option<u64>,
    cache_task: Option<Task<Result<u64>>>,

    unlock_bg_disable_btn: DRectButton,
    unlock_bg_choose_btn: DRectButton,
    unlock_bgm_disable_btn: DRectButton,
    unlock_bgm_choose_btn: DRectButton,
}

impl GeneralList {
    pub fn new(icon_lang: SafeTexture) -> Self {
        let mut this = Self {
            icon_lang,

            lang_btn: ChooseButton::new()
                .with_options(LANG_NAMES.iter().map(|s| s.to_string()).collect())
                .with_selected(
                    get_data()
                        .language
                        .as_ref()
                        .and_then(|it| it.parse::<LanguageIdentifier>().ok())
                        .and_then(|ident| LANG_IDENTS.iter().position(|it| *it == ident))
                        .unwrap_or_default(),
                ),

            #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
            fullscreen_btn: DRectButton::new(),

            cache_btn: DRectButton::new(),
            offline_btn: DRectButton::new(),
            server_status_btn: DRectButton::new(),
            mp_btn: DRectButton::new(),
            mp_addr_btn: DRectButton::new(),
            #[cfg(not(target_env = "ohos"))]
            lowq_btn: DRectButton::new(),
            prefer_reduced_motion_btn: DRectButton::new(),
            insecure_btn: DRectButton::new(),
            enable_anys_btn: DRectButton::new(),
            anys_gateway_btn: DRectButton::new(),

            cache_size: None,
            cache_task: None,

            unlock_bg_disable_btn: DRectButton::new(),
            unlock_bg_choose_btn: DRectButton::new(),
            unlock_bgm_disable_btn: DRectButton::new(),
            unlock_bgm_choose_btn: DRectButton::new(),
        };
        let _ = this.update_cache_size();
        this
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.lang_btn.top_touch(touch, t)
    }

    fn dir_size(path: impl Into<PathBuf>) -> io::Result<u64> {
        fn inner(mut dir: fs::ReadDir) -> io::Result<u64> {
            dir.try_fold(0, |acc, file| {
                let file = file?;
                let size = match file.metadata()? {
                    data if data.is_dir() => inner(fs::read_dir(file.path())?)?,
                    data => data.len(),
                };
                Ok(acc + size)
            })
        }

        inner(fs::read_dir(path.into())?)
    }

    fn update_cache_size(&mut self) -> Result<()> {
        self.cache_size = None;
        let cache_dir = dir::cache()?;
        self.cache_task = Some(Task::new(async { Ok(Self::dir_size(cache_dir)?) }));
        Ok(())
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;

        if self.lang_btn.touch(touch, t) {
            return Ok(Some(false));
        }

        #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
        if self.fullscreen_btn.touch(touch, t) {
            config.fullscreen_mode ^= true;
            macroquad::window::set_fullscreen(config.fullscreen_mode);
            return Ok(Some(true));
        }

        if self.cache_btn.touch(touch, t) {
            fs::remove_dir_all(dir::cache()?)?;
            self.update_cache_size()?;
            show_message(tl!("item-cache-cleared")).ok();
            return Ok(Some(false));
        }
        if self.offline_btn.touch(touch, t) {
            config.offline_mode ^= true;
            return Ok(Some(true));
        }
        if self.server_status_btn.touch(touch, t) {
            let _ = open_url(STATUS_PAGE);
            return Ok(Some(true));
        }
        if self.mp_btn.touch(touch, t) {
            config.mp_enabled ^= true;
            return Ok(Some(true));
        }
        if self.mp_addr_btn.touch(touch, t) {
            request_input("mp_addr", InputBox::new().default_text(&config.mp_address));
            return Ok(Some(true));
        }
        #[cfg(not(target_env = "ohos"))]
        if self.lowq_btn.touch(touch, t) {
            config.sample_count = if config.sample_count == 1 { 2 } else { 1 };
            return Ok(Some(true));
        }
        if self.prefer_reduced_motion_btn.touch(touch, t) {
            data.prefer_reduced_motion ^= true;
            PREFER_REDUCED_MOTION.store(data.prefer_reduced_motion, Ordering::Relaxed);
            return Ok(Some(true));
        }
        if self.insecure_btn.touch(touch, t) {
            data.accept_invalid_cert ^= true;
            return Ok(Some(true));
        }
        if self.enable_anys_btn.touch(touch, t) {
            data.enable_anys ^= true;
            return Ok(Some(true));
        }
        if self.anys_gateway_btn.touch(touch, t) {
            request_input("anys_gateway", InputBox::new().default_text(&data.anys_gateway));
            return Ok(Some(true));
        }

        {
            use crate::unlock::{get_state, set_state, FEAT_BG_CHANGE, FEAT_BGM_CHANGE};
            if FEAT_BG_CHANGE.load(Ordering::Relaxed) {
                if self.unlock_bg_disable_btn.touch(touch, t) {
                    let mut s = get_state(); s.bg_change = false; s.bg_path = None; set_state(s);
                    save_unlock();
                    return Ok(Some(true));
                }
                if self.unlock_bg_choose_btn.touch(touch, t) {
                    xcsim_core::scene::request_file("_unlock_bg");
                    return Ok(Some(false));
                }
            }
            if FEAT_BGM_CHANGE.load(Ordering::Relaxed) {
                if self.unlock_bgm_disable_btn.touch(touch, t) {
                    let mut s = get_state(); s.bgm_change = false; s.bgm_path = None; set_state(s);
                    save_unlock();
                    return Ok(Some(true));
                }
                if self.unlock_bgm_choose_btn.touch(touch, t) {
                    xcsim_core::scene::request_file("_unlock_bgm");
                    return Ok(Some(false));
                }
            }
        }

        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        self.lang_btn.update(t);

        let data = get_data_mut();
        if self.lang_btn.changed() {
            data.language = Some(LANG_IDENTS[self.lang_btn.selected()].to_string());
            sync_data();
            return Ok(true);
        }

        if let Some(task) = &mut self.cache_task {
            if let Some(res) = task.take() {
                self.cache_size = res.ok();
                self.cache_task = None;
            }
        }

        if let Some((id, text)) = take_input() {
            if id == "mp_addr" {
                if let Err(err) = text.to_socket_addrs() {
                    show_error(anyhow::Error::new(err).context(tl!("item-mp-addr-invalid")));
                    return Ok(false);
                } else {
                    data.config.mp_address = text;
                    return Ok(true);
                }
            } else if id == "anys_gateway" {
                if let Err(err) = Url::parse(&text) {
                    show_error(anyhow::Error::new(err).context(tl!("item-anys-gateway-invalid")));
                    return Ok(false);
                } else {
                    data.anys_gateway = text;
                    return Ok(true);
                }
            } else {
                return_input(id, text);
            }
        }

        if let Some((id, file)) = xcsim_core::scene::take_file() {
            match id.as_str() {
                "_unlock_bg" => {
                    let mut s = crate::unlock::get_state(); s.bg_path = Some(file); crate::unlock::set_state(s);
                    save_unlock();
                    return Ok(true);
                }
                "_unlock_bgm" => {
                    let mut s = crate::unlock::get_state(); s.bgm_path = Some(file); crate::unlock::set_state(s);
                    save_unlock();
                    return Ok(true);
                }
                _ => xcsim_core::scene::return_file(id, file),
            }
        }

        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;

        item! {
            render_title(ui, tl!("item-language"), None);
            self.lang_btn.render(ui, rr, t);
        }
        #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
        item! {
            render_title(ui, tl!("item-fullscreen"), None);
            render_switch(ui, rr, t, &mut self.fullscreen_btn, config.fullscreen_mode);
        }
        item! {
            render_title(ui, tl!("item-offline"), Some(tl!("item-offline-sub")));
            render_switch(ui, rr, t, &mut self.offline_btn, config.offline_mode);
        }
        item! {
            render_title(ui, tl!("item-mp"), Some(tl!("item-mp-sub")));
            render_switch(ui, rr, t, &mut self.mp_btn, config.mp_enabled);
        }
        item! {
            render_title(ui, tl!("item-mp-addr"), Some(tl!("item-mp-addr-sub")));
            render_textfield(ui, rr, t, &mut self.mp_addr_btn, &config.mp_address, 0.4);
        }
        item! {
            render_title(ui, tl!("item-server-status"), Some(tl!("item-server-status-sub")));
            self.server_status_btn.render_text(ui, rr, t, tl!("check-status"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-prefer-reduced-motion"), Some(tl!("item-prefer-reduced-motion-sub")));
            render_switch(ui, rr, t, &mut self.prefer_reduced_motion_btn, data.prefer_reduced_motion);
        }
        #[cfg(not(target_env = "ohos"))]
        item! {
            render_title(ui, tl!("item-lowq"), Some(tl!("item-lowq-sub")));
            render_switch(ui, rr, t, &mut self.lowq_btn, config.sample_count == 1);
        }
        item! {
            let cache_size = if let Some(size) = self.cache_size {
                Cow::Owned(tl!("item-cache-size", "size" => ByteSize(size).to_string()))
            } else {
                tl!("item-cache-size-loading")
            };
            render_title(ui, tl!("item-clear-cache"), Some(cache_size));
            self.cache_btn.render_text(ui, rr, t, tl!("item-clear-cache-btn"), 0.5, true);
        }
        ui.dy(0.04);
        h += 0.04;
        item! {
            render_title(ui, tl!("item-insecure"), Some(tl!("item-insecure-sub")));
            render_switch(ui, rr, t, &mut self.insecure_btn, data.accept_invalid_cert);
        }
        item! {
            render_title(ui, tl!("item-enable-anys"), Some(tl!("item-enable-anys-sub")));
            render_switch(ui, rr, t, &mut self.enable_anys_btn, data.enable_anys);
        }
        item! {
            render_title(ui, tl!("item-anys-gateway"), Some(tl!("item-anys-gateway-sub")));
            render_textfield(ui, rr, t, &mut self.anys_gateway_btn, &data.anys_gateway, 0.4);
        }

        {
            use crate::unlock::{get_state, FEAT_BG_CHANGE, FEAT_BGM_CHANGE};
            use std::sync::atomic::Ordering;
            let bg  = FEAT_BG_CHANGE.load(Ordering::Relaxed);
            let bgm = FEAT_BGM_CHANGE.load(Ordering::Relaxed);
            if bg || bgm {
                let state = get_state();
                ui.dy(0.06);
                h += 0.06;
                let sep_r = ui.text("── Unlock Features ──")
                    .pos(w * 0.5, 0.).anchor(0.5, 0.).no_baseline()
                    .size(0.38).color(xcsim_core::ext::semi_white(0.5)).draw();
                ui.dy(sep_r.h + 0.04);
                h += sep_r.h + 0.04;

                if bg {
                    item! {
                        let bg_label = state.bg_path.as_deref().unwrap_or("(none)");
                        render_title(ui, "Background", Some(Cow::Owned(bg_label.to_owned())));
                        let half_rr = Rect::new(rr.x, rr.y, (rr.w - 0.02) / 2., rr.h);
                        self.unlock_bg_choose_btn.render_text(ui, half_rr, t, "Choose", 0.43, true);
                        let dis_rr = Rect::new(half_rr.right() + 0.02, rr.y, half_rr.w, rr.h);
                        self.unlock_bg_disable_btn.render_text(ui, dis_rr, t, "Disable", 0.43, true);
                    }
                }
                if bgm {
                    item! {
                        let bgm_label = state.bgm_path.as_deref().unwrap_or("(none)");
                        render_title(ui, "BGM", Some(Cow::Owned(bgm_label.to_owned())));
                        let half_rr = Rect::new(rr.x, rr.y, (rr.w - 0.02) / 2., rr.h);
                        self.unlock_bgm_choose_btn.render_text(ui, half_rr, t, "Choose", 0.43, true);
                        let dis_rr = Rect::new(half_rr.right() + 0.02, rr.y, half_rr.w, rr.h);
                        self.unlock_bgm_disable_btn.render_text(ui, dis_rr, t, "Disable", 0.43, true);
                    }
                }
            }
        }

        self.lang_btn.render_top(ui, t, 1.);
        (w, h)
    }
}

struct AudioList {
    adjust_btn: DRectButton,
    music_slider: Slider,
    sfx_slider: Slider,
    bgm_slider: Slider,
    cali_btn: DRectButton,
    #[cfg(not(target_os = "android"))]
    preferred_sample_rate_btn: DRectButton,
    #[cfg(target_env = "ohos")]
    audio_buffer_size_btn: DRectButton,

    cali_task: LocalTask<Result<OffsetPage>>,
    next_page: Option<NextPage>,
}

impl AudioList {
    pub fn new() -> Self {
        Self {
            adjust_btn: DRectButton::new(),
            music_slider: Slider::new(0.0..2.0, 0.05),
            sfx_slider: Slider::new(0.0..2.0, 0.05),
            bgm_slider: Slider::new(0.0..2.0, 0.05),
            cali_btn: DRectButton::new(),
            #[cfg(not(target_os = "android"))]
            preferred_sample_rate_btn: DRectButton::new(),
            #[cfg(target_env = "ohos")]
            audio_buffer_size_btn: DRectButton::new(),

            cali_task: None,
            next_page: None,
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.adjust_btn.touch(touch, t) {
            config.adjust_time ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.music_slider.touch(touch, t, &mut config.volume_music) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.sfx_slider.touch(touch, t, &mut config.volume_sfx) {
            return Ok(wt);
        }
        let old = config.volume_bgm;
        if let wt @ Some(_) = self.bgm_slider.touch(touch, t, &mut config.volume_bgm) {
            if (config.volume_bgm - old).abs() > 0.001 {
                BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
            }
            return Ok(wt);
        }
        if self.cali_btn.touch(touch, t) {
            self.cali_task = Some(Box::pin(OffsetPage::new()));
            return Ok(Some(false));
        }
        #[cfg(not(target_os = "android"))]
        if self.preferred_sample_rate_btn.touch(touch, t) {
            let options = [None, Some(44100), Some(48000), Some(88200), Some(96000), Some(192000)];
            let current = config.preferred_sample_rate;
            let selected = options.iter().position(|&r| r == current).unwrap_or(0);
            config.preferred_sample_rate = options[(selected + 1) % options.len()];
            return Ok(Some(true));
        }
        #[cfg(target_env = "ohos")]
        if self.audio_buffer_size_btn.touch(touch, t) {
            let options = [128u32, 256u32, 512u32];
            let current = config.audio_buffer_size.unwrap_or(256);
            let selected = options.iter().position(|&r| r == current).unwrap_or(1);
            config.audio_buffer_size = Some(options[(selected + 1) % options.len()]);
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        if let Some(task) = &mut self.cali_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => show_error(err.context(tl!("load-cali-failed"))),
                    Ok(page) => {
                        self.next_page = Some(NextPage::Overlay(Box::new(page)));
                    }
                }
                self.cali_task = None;
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-adjust"), Some(tl!("item-adjust-sub")));
            render_switch(ui, rr, t, &mut self.adjust_btn, config.adjust_time);
        }
        item! {
            render_title(ui, tl!("item-music"), None);
            self.music_slider.render(ui, rr, t, config.volume_music, format!("{:.2}", config.volume_music));
        }
        item! {
            render_title(ui, tl!("item-sfx"), None);
            self.sfx_slider.render(ui, rr, t, config.volume_sfx, format!("{:.2}", config.volume_sfx));
        }
        item! {
            render_title(ui, tl!("item-bgm"), None);
            self.bgm_slider.render(ui, rr, t, config.volume_bgm, format!("{:.2}", config.volume_bgm));
        }
        item! {
            render_title(ui, tl!("item-cali"), None);
            self.cali_btn.render_text(ui, rr, t, format!("{:.0}ms", config.offset * 1000.), 0.5, true);
        }
        #[cfg(not(target_os = "android"))]
        item! {
            render_title(ui, tl!("item-preferred-sample-rate"), None);
            let text = if let Some(rate) = config.preferred_sample_rate {
                format!("{} Hz", rate)
            } else {
                tl!("preferred-sample-rate-default").to_string()
            };
            self.preferred_sample_rate_btn.render_text(ui, rr, t, text, 0.5, false);
        }
        #[cfg(target_env = "ohos")]
        item! {
            render_title(ui, tl!("item-audio-buffer-size"), None);
            let buf_size = config.audio_buffer_size.unwrap_or(256);
            self.audio_buffer_size_btn.render_text(ui, rr, t, format!("{}", buf_size), 0.5, false);
        }
        (w, h)
    }

    pub fn next_page(&mut self) -> Option<NextPage> {
        self.next_page.take()
    }
}

struct ChartList {
    show_acc_btn: DRectButton,
    ap_fc_indicator_btn: DRectButton,
    show_avg_fps_btn: DRectButton,
    dc_pause_btn: DRectButton,
    dhint_btn: DRectButton,
    opt_btn: DRectButton,
    use_keyboard_btn: DRectButton,
    strict_btn: DRectButton,
    arcscore_btn: DRectButton,
    hp_bar_btn: DRectButton,
    ui_choose_btn: ChooseButton,
    speed_slider: Slider,
    size_slider: Slider,
}

impl ChartList {
    pub fn new() -> Self {
        let initial = if get_data().config.use_classic_ui { 1 } else { 0 };
        Self {
            show_acc_btn: DRectButton::new(),
            ap_fc_indicator_btn: DRectButton::new(),
            show_avg_fps_btn: DRectButton::new(),
            dc_pause_btn: DRectButton::new(),
            dhint_btn: DRectButton::new(),
            opt_btn: DRectButton::new(),
            use_keyboard_btn: DRectButton::new(),
            strict_btn: DRectButton::new(),
            arcscore_btn: DRectButton::new(),
            hp_bar_btn: DRectButton::new(),
            ui_choose_btn: ChooseButton::new()
                .with_options(vec!["XHUS UI".to_string(), "Phigros".to_string()])
                .with_selected(initial),
            speed_slider: Slider::new(0.5..2., 0.05),
            size_slider: Slider::new(0.8..1.2, 0.005),
        }
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.ui_choose_btn.top_touch(touch, t)
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.show_acc_btn.touch(touch, t) {
            config.show_acc ^= true;
            return Ok(Some(true));
        }
        if self.ap_fc_indicator_btn.touch(touch, t) {
            config.ap_fc_indicator ^= true;
            return Ok(Some(true));
        }
        if self.show_avg_fps_btn.touch(touch, t) {
            config.show_avg_fps ^= true;
            return Ok(Some(true));
        }
        if self.dc_pause_btn.touch(touch, t) {
            config.double_click_to_pause ^= true;
            return Ok(Some(true));
        }
        if self.dhint_btn.touch(touch, t) {
            config.double_hint ^= true;
            return Ok(Some(true));
        }
        if self.opt_btn.touch(touch, t) {
            config.aggressive ^= true;
            return Ok(Some(true));
        }
        if self.use_keyboard_btn.touch(touch, t) {
            config.use_keyboard ^= true;
            return Ok(Some(true));
        }
        if self.strict_btn.touch(touch, t) {
            config.strict ^= true;
            return Ok(Some(true));
        }
        if self.arcscore_btn.touch(touch, t) {
            config.strict ^= true;
            return Ok(Some(true));
        }
        if self.hp_bar_btn.touch(touch, t) {
            config.hp_bar_enabled ^= true;
            return Ok(Some(true));
        }
        if self.ui_choose_btn.touch(touch, t) {
            return Ok(Some(false));
        }
        if let wt @ Some(_) = self.speed_slider.touch(touch, t, &mut config.speed) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.size_slider.touch(touch, t, &mut config.note_scale) {
            return Ok(wt);
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        self.ui_choose_btn.update(t);
        if self.ui_choose_btn.changed() {
            let data = get_data_mut();
            data.config.use_classic_ui = self.ui_choose_btn.selected() == 1;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-show-acc"), None);
            render_switch(ui, rr, t, &mut self.show_acc_btn, config.show_acc);
        }
        item! {
            render_title(ui, tl!("item-ap-fc-indicator"), Some(tl!("item-ap-fc-indicator-sub")));
            render_switch(ui, rr, t, &mut self.ap_fc_indicator_btn, config.ap_fc_indicator);
        }
        item! {
            render_title(ui, tl!("item-show-avg-fps"), None);
            render_switch(ui, rr, t, &mut self.show_avg_fps_btn, config.show_avg_fps);
        }
        item! {
            render_title(ui, tl!("item-dc-pause"), None);
            render_switch(ui, rr, t, &mut self.dc_pause_btn, config.double_click_to_pause);
        }
        item! {
            render_title(ui, tl!("item-dhint"), Some(tl!("item-dhint-sub")));
            render_switch(ui, rr, t, &mut self.dhint_btn, config.double_hint);
        }
        item! {
            render_title(ui, tl!("item-opt"), Some(tl!("item-opt-sub")));
            render_switch(ui, rr, t, &mut self.opt_btn, config.aggressive);
        }
        item! {
            render_title(ui, tl!("item-use-keyboard"), Some(tl!("item-use-keyboard-sub")));
            render_switch(ui, rr, t, &mut self.use_keyboard_btn, config.use_keyboard);
        }
        item! {
            render_title(ui, Cow::Borrowed("HP bar"), Some(Cow::Borrowed("Show a Tempest-style HP gauge during gameplay. The chart ends when HP reaches 0.")));
            render_switch(ui, rr, t, &mut self.hp_bar_btn, config.hp_bar_enabled);
        }
        item! {
            render_title(ui, Cow::Borrowed("Gameplay UI"), None);
            self.ui_choose_btn.render(ui, rr, t);
        }
        item! {
            render_title(ui, tl!("item-speed"), None);
            self.speed_slider.render(ui, rr, t, config.speed, format!("{:.2}", config.speed));
        }
        item! {
            render_title(ui, tl!("item-note-size"), None);
            self.size_slider.render(ui, rr, t, config.note_scale, format!("{:.3}", config.note_scale));
        }
        item! {
            render_title(ui, tl!("strict-judge"), Some(tl!("xhgrjimpunsiuzi1")));
            render_switch(ui, rr, t, &mut self.strict_btn, config.strict);
        }
        item! {
            render_title(ui, tl!("strict-judge-alt"), Some(tl!("xhgrjimpunsiuzi1")));
            render_switch(ui, rr, t, &mut self.arcscore_btn, config.strict);
        }
        self.ui_choose_btn.render_top(ui, t, 1.);
        (w, h)
    }
}

struct DebugList {
    chart_debug_btn: DRectButton,
    touch_debug_btn: DRectButton,
}

impl DebugList {
    pub fn new() -> Self {
        Self {
            chart_debug_btn: DRectButton::new(),
            touch_debug_btn: DRectButton::new(),
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.chart_debug_btn.touch(touch, t) {
            config.chart_debug ^= true;
            return Ok(Some(true));
        }
        if self.touch_debug_btn.touch(touch, t) {
            config.touch_debug ^= true;
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-chart-debug"), Some(tl!("item-chart-debug-sub")));
            render_switch(ui, rr, t, &mut self.chart_debug_btn, config.chart_debug);
        }
        item! {
            render_title(ui, tl!("item-touch-debug"), Some(tl!("item-touch-debug-sub")));
            render_switch(ui, rr, t, &mut self.touch_debug_btn, config.touch_debug);
        }
        (w, h)
    }
}

struct CustomList {
    watermark_btn: DRectButton,
    judge_btn: DRectButton,
    texture_pack_format_btn: DRectButton,
    watermark_text_btn: DRectButton,
    combo_text_btn: DRectButton,
    playerrks_btn: DRectButton,
    playername_btn: DRectButton,
    size_slider: Slider,
    chart_ratio_slider: Slider,
    r_slider: Slider,
    g_slider: Slider,
    b_slider: Slider,
    ui_scale_slider: Slider,
    style_reload_btn: DRectButton,
    style_disable_btn: DRectButton,
    style_delete_btn: DRectButton,
    style_status: String,
}

impl CustomList {
    pub fn new() -> Self {
        let initial_status = if xcsim_core::custom_style::is_enabled() {
            "Active".to_string()
        } else {
            "Not loaded".to_string()
        };
        Self {
            watermark_btn: DRectButton::new(),
            judge_btn: DRectButton::new(),
            texture_pack_format_btn: DRectButton::new(),
            watermark_text_btn: DRectButton::new(),
            combo_text_btn: DRectButton::new(),
            playerrks_btn: DRectButton::new(),
            playername_btn: DRectButton::new(),
            size_slider: Slider::new(0.25..0.8, 0.01),
            chart_ratio_slider: Slider::new(0.05..1.0, 0.05),
            r_slider: Slider::new(0.0..1.0, 0.01),
            g_slider: Slider::new(0.0..1.0, 0.01),
            b_slider: Slider::new(0.0..1.0, 0.01),
            ui_scale_slider: Slider::new(0.4..1.0, 0.05),
            style_reload_btn: DRectButton::new(),
            style_disable_btn: DRectButton::new(),
            style_delete_btn: DRectButton::new(),
            style_status: initial_status,
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;

        if self.texture_pack_format_btn.touch(touch, t) {
            request_input(
                "texture_pack_format",
                InputBox::new().default_text(&config.texture_pack_format),
            );
            return Ok(Some(true));
        }
        if self.watermark_btn.touch(touch, t) {
            config.watermark_enabled ^= true;
            return Ok(Some(true));
        }
        if self.judge_btn.touch(touch, t) {
            config.show_judge_text ^= true;
            return Ok(Some(true));
        }
        if self.watermark_text_btn.touch(touch, t) {
            request_input(
                "watermark_text",
                InputBox::new().default_text(&config.watermark_text),
            );
            return Ok(Some(true));
        }
        if self.playerrks_btn.touch(touch, t) {
            request_input(
                "player_rks",
                InputBox::new().default_text(&format!("{:.2}", config.player_rks)),
            );
            return Ok(Some(true));
        }
        if self.playername_btn.touch(touch, t) {
            request_input(
                "player_name",
                InputBox::new().default_text(&config.player_name),
            );
            return Ok(Some(true));
        }
        if self.combo_text_btn.touch(touch, t) {
            request_input(
                "combo_text",
                InputBox::new().default_text(&config.combotext),
            );
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.size_slider.touch(touch, t, &mut config.watermark_size) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.r_slider.touch(touch, t, &mut config.watermark_r) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.g_slider.touch(touch, t, &mut config.watermark_g) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.b_slider.touch(touch, t, &mut config.watermark_b) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.ui_scale_slider.touch(touch, t, &mut config.ui_scale) {
            return Ok(wt);
        }
        if self.style_reload_btn.touch(touch, t) {
            xcsim_core::scene::request_file("_style_import");
            self.style_status = "Waiting for file...".to_string();
            return Ok(Some(false));
        }
        if self.style_disable_btn.touch(touch, t) {
            crate::custom_style_io::disable_active_style();
            self.style_status = "Disabled".to_string();
            return Ok(Some(false));
        }
        if self.style_delete_btn.touch(touch, t) {
            let _ = crate::custom_style_io::delete_xml_on_disk();
            crate::custom_style_io::disable_active_style();
            self.style_status = "Removed".to_string();
            return Ok(Some(false));
        }

        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        let data = get_data_mut();
        let config = &mut data.config;

        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "watermark_text" => {
                    config.watermark_text = text;
                    return Ok(true);
                }
                "player_rks" => {
                    if let Ok(v) = text.parse::<f32>() {
                        config.player_rks = v;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                "combo_text" => {
                    config.combotext = text;
                    return Ok(true);
                }
                "player_name" => {
                    config.player_name = text;
                    return Ok(true);
                }
                "texture_pack_format" => {
                    let v = text.trim().to_string();
                    if !v.is_empty() {
                        config.texture_pack_format = v;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                _ => return_input(id, text),
            }
        }

        if let Some((id, file)) = xcsim_core::scene::take_file() {
            match id.as_str() {
                "_style_import" => {
                    let result = (|| -> Result<String> {
                        let xml = std::fs::read_to_string(&file)
                            .map_err(|e| anyhow::anyhow!("read failed: {}", e))?;
                        let style = crate::custom_style_io::parse_xml(&xml)?;
                        crate::custom_style_io::write_xml_to_disk(&xml)?;
                        xcsim_core::custom_style::apply(style);
                        let name = std::path::Path::new(&file)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "style".to_string());
                        Ok(name)
                    })();
                    self.style_status = match result {
                        Ok(name) => format!("Active: {}", name),
                        Err(err) => format!("Error: {}", err),
                    };
                    return Ok(false);
                }
                _ => xcsim_core::scene::return_file(id, file),
            }
        }

        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let cfg = &get_data().config;

        item! {
            render_title(ui, tl!("watermark"), Some(tl!("watermark-sub")));
            render_switch(ui, rr, t, &mut self.watermark_btn, cfg.watermark_enabled);
        }
        item! {
            render_title(ui, tl!("texture-pack-format"), Some(tl!("texture-pack-format-sub")));
            render_textfield(ui, rr, t, &mut self.texture_pack_format_btn, &cfg.texture_pack_format, 0.4);
        }
        item! {
            render_title(ui, tl!("judgetext"), None);
            render_switch(ui, rr, t, &mut self.judge_btn, cfg.show_judge_text);
        }
        item! {
            render_title(ui, tl!("watermark-text"), None);
            render_textfield(ui, rr, t, &mut self.watermark_text_btn, &cfg.watermark_text, 0.4);
        }
        item! {
            render_title(ui, tl!("player-rks"), None);
            render_textfield(ui, rr, t, &mut self.playerrks_btn, format!("{:.2}", cfg.player_rks), 0.4);
        }
        item! {
            render_title(ui, tl!("player-name"), None);
            render_textfield(ui, rr, t, &mut self.playername_btn, &cfg.player_name, 0.4);
        }
        item! {
            render_title(ui, tl!("combo-text"), Some(tl!("ccb")));
            render_textfield(ui, rr, t, &mut self.combo_text_btn, &cfg.combotext, 0.4);
        }
        item! {
            render_title(ui, tl!("watermark-size"), None);
            self.size_slider.render(ui, rr, t, cfg.watermark_size, format!("{:.0}px", cfg.watermark_size));
        }
        item! {
            render_title(ui, "R", None);
            self.r_slider.render(ui, rr, t, cfg.watermark_r, format!("{:.2}", cfg.watermark_r));
        }
        item! {
            render_title(ui, "G", None);
            self.g_slider.render(ui, rr, t, cfg.watermark_g, format!("{:.2}", cfg.watermark_g));
        }
        item! {
            render_title(ui, "B", None);
            self.b_slider.render(ui, rr, t, cfg.watermark_b, format!("{:.2}", cfg.watermark_b));
        }
        item! {
            render_title(ui, "UI & Chart Scale", Some(Cow::Borrowed("Scale the entire UI and chart. 1.0 = full size")));
            self.ui_scale_slider.render(ui, rr, t, cfg.ui_scale, format!("{:.0}%", cfg.ui_scale * 100.));
        }
        item! {
            render_title(ui, Cow::Borrowed("Style"), Some(Cow::Owned(self.style_status.clone())));
        }
        item! {
            render_title(ui, Cow::Borrowed("  Import XML"), None);
            render_textfield(ui, rr, t, &mut self.style_reload_btn, "Import", 0.4);
        }
        item! {
            render_title(ui, Cow::Borrowed("  Disable"), None);
            render_textfield(ui, rr, t, &mut self.style_disable_btn, "Disable", 0.4);
        }
        item! {
            render_title(ui, Cow::Borrowed("  Remove"), None);
            render_textfield(ui, rr, t, &mut self.style_delete_btn, "Remove", 0.4);
        }

        (w, h)
    }
}
