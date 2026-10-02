#![allow(unused)]

xcsim_core_l10n::tl_file!("game");

use super::{
    draw_background,
    ending::RecordUpdateState,
    loading::{BasicPlayer, UpdateFn, UploadFn, SaveFn},
    request_input, return_input, show_message, take_input, EndingScene, NextScene, Scene,
};
use crate::{
    bin::BinaryReader,
    config::{Config, Mods},
    core::{copy_fbo, BadNote, Chart, ChartExtra, Effect, Point, Resource, UIElement, Vector, PGR_FONT},
    ext::{parse_time, screen_aspect, semi_white, RectExt, SafeTexture, ScaleType},
    fs::FileSystem,
    info::{ChartFormat, ChartInfo},
    judge::Judge,
    parse::{parse_extra, parse_pec, parse_phigros, parse_rpe},
    task::Task,
    time::TimeManager,
    ui::{RectButton, TextPainter, Ui},
};
use anyhow::{bail, Context, Result};
use concat_string::concat_string;
use lyon::path::Path;
use macroquad::{prelude::*, window::InternalGlContext};
use sasa::{Music, MusicParams};
use serde::{Deserialize, Serialize};
use inputbox::InputBox;
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{Cursor, ErrorKind},
    ops::{Deref, DerefMut, Range},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{debug, warn};

const PAUSE_CLICK_INTERVAL: f32 = 0.7;

const HUD_HIGHLIGHT_DURATION: f32 = PAUSE_CLICK_INTERVAL;

const HUD_ENTER_DURATION: f32 = 0.6;

const HUD_SLIDE_DIST_X: f32 = 0.55;

const HUD_SLIDE_DIST_Y: f32 = 0.4;

#[cfg(feature = "closed")]
mod inner;
#[cfg(feature = "closed")]
use inner::*;

const WAIT_TIME: f32 = 0.5;
const AFTER_TIME: f32 = 0.7;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleRecord {
    pub score: i32,
    pub accuracy: f32,
    pub full_combo: bool,
}

impl SimpleRecord {
    pub fn update(&mut self, other: &SimpleRecord) -> bool {
        let mut changed = false;
        if other.score > self.score {
            self.score = other.score;
            changed = true;
        }
        if other.accuracy > self.accuracy {
            self.accuracy = other.accuracy;
            changed = true;
        }
        if other.full_combo & !self.full_combo {
            self.full_combo = other.full_combo;
            changed = true;
        }
        changed
    }
}

fn fmt_time(t: f32) -> String {
    let f = t < 0.;
    let t = t.abs();
    let secs = t % 60.;
    let mut t = (t / 60.) as u64;
    let mins = t % 60;
    t /= 60;
    let hrs = t % 100;
    format!("{}{hrs:02}:{mins:02}:{secs:05.2}", if f { "-" } else { "" })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    fn on_game_start();
}

#[derive(PartialEq, Eq)]
pub enum GameMode {
    Normal,
    TweakOffset,
    Exercise,
    NoRetry,
    View,
}

#[derive(Clone)]
enum State {
    Starting,
    BeforeMusic,
    Playing,
    Ending,
}

pub struct GameScene {
    should_exit: bool,
    next_scene: Option<NextScene>,
    perfect_no_miss_start: Option<f32>,

    pub mode: GameMode,
    pub res: Resource,
    pub chart: Chart,
    pub judge: Judge,
        display_score: u32,
    score_anim_from: u32,
    score_anim_to: u32,
    score_anim_start: f64,
    score_anim_duration: f64,
    pub gl: InternalGlContext<'static>,
    player: Option<BasicPlayer>,
    chart_bytes: Vec<u8>,
    chart_format: ChartFormat,
    info_offset: f32,
    effects: Vec<Effect>,
    perfect_fade_start: Option<f32>,
    perfect_flash_start: Option<f32>,
    pure_memory_start: Option<f32>,

    first_in: bool,
    exercise_range: Range<f32>,
    exercise_press: Option<(i8, u64)>,
    exercise_btns: (RectButton, RectButton),

    pub music: Music,

    state: State,
    pub last_update_time: f64,
    pause_rewind: Option<f64>,
    pause_first_time: f32,

    pub pause_disabled: bool,

    highlight_start: f32,

    pub bad_notes: Vec<BadNote>,

    upload_fn: Option<UploadFn>,
    update_fn: Option<UpdateFn>,
    save_fn: Option<SaveFn>,

    best_record: Option<SimpleRecord>,

    pub touch_points: Vec<(f32, f32)>,
    fps_frame_count: u32,
    fps_total_time: f64,
    fps_last_frame_time: f64,

    dead: bool,

    hp_enabled: bool,
    hp_value: f32,
    hp_rate_offset: f32,
    hp_last_counts: [u32; 5],
    hp_max_score: u32,
    hp_dead: bool,
    hp_red_blend: f32,
    hp_black_blend: f32,
    hp_prev_value: f32,
    hp_n: u32,
    hp_consec_miss: u32,
    hp_doom_drain: bool,

    skip_hold_start: Option<f64>,
    skip_by_touch: bool,
    skip_anim_start: Option<f64>,
}

const SKIP_HOLD_TIME: f64 = 3.0;
const SKIP_ANIM_TIME: f64 = 1.4;

const HP_IMPULSE_TAU: f32 = 1.0;

const HP_L: f32 = 0.0008;

const HP_PERFECT: f32 = 8.0;
const HP_GOOD:    f32 = -5.0;
const HP_MISS:    f32 = -10.0;

const HP_GAIN_CAP: f32 = 2.0;

const HP_LOCK_STREAK: u32 = 6;
const HP_DOOM_RATE:   f32 = -10.0;

macro_rules! reset {
    ($self:ident, $res:expr, $tm:ident) => {{
        $self.bad_notes.clear();
        $self.judge.reset();
        $self.chart.reset();
        $res.judge_line_color = Color::from_hex_argb($res.res_pack.info.color_perfect);
        $self.music.pause()?;
        $self.music.seek_to(0.)?;
        $tm.speed = $res.config.speed as _;
        $tm.reset();
        $self.last_update_time = $tm.now();
        $self.state = State::Starting;
        $self.fps_frame_count = 0;
        $self.fps_total_time = 0.0;
        $self.fps_last_frame_time = $tm.real_time();
        $self.dead = false;
        $self.hp_value = 100.0;
        $self.hp_rate_offset = 0.0;
        $self.hp_last_counts = [0; 5];
        $self.hp_max_score = 0;
        $self.hp_dead = false;
        $self.hp_red_blend = 0.0;
        $self.hp_black_blend = 0.0;
        $self.hp_prev_value = 100.0;
        $self.hp_consec_miss = 0;
        $self.hp_doom_drain = false;

        $self.skip_hold_start = None;
        $self.skip_by_touch = false;
        $self.skip_anim_start = None;

        $self.highlight_start = $tm.real_time() as f32;
    }};
}
impl GameScene {
    pub const BEFORE_TIME: f32 = 0.7;
    pub const FADEOUT_TIME: f32 = WAIT_TIME + AFTER_TIME + 0.3;

    pub async fn load_chart_bytes(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<Vec<u8>> {
        if let Ok(bytes) = fs.load_file(&info.chart).await {
            return Ok(bytes);
        }
        if let Some(name) = info.chart.strip_suffix(".pec") {
            if let Ok(bytes) = fs.load_file(&concat_string!(name, ".json")).await {
                return Ok(bytes);
            }
        }
        bail!("Cannot find chart file")
    }
    pub fn infer_chart_format(info: &ChartInfo, bytes: &[u8]) -> ChartFormat {
        info.format.clone().unwrap_or_else(|| {
            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                if text.starts_with('{') {
                    if text.contains("\"META\"") {
                        ChartFormat::Rpe
                    } else {
                        ChartFormat::Pgr
                    }
                } else {
                    ChartFormat::Pec
                }
            } else {
                ChartFormat::Pbc
            }
        })
    }

    pub async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<(Chart, Vec<u8>, ChartFormat)> {
        let extra = fs.load_file("extra.json").await.ok().map(String::from_utf8).transpose()?;
        let extra = if let Some(extra) = extra {
            parse_extra(&extra, fs).await.context("Failed to parse extra")?
        } else {
            ChartExtra::default()
        };
        let bytes = Self::load_chart_bytes(fs, info).await.context("Failed to load chart")?;
        let format = info.format.clone().unwrap_or_else(|| {
            if let Ok(text) = String::from_utf8(bytes.clone()) {
                if text.starts_with('{') {
                    if text.contains("\"META\"") {
                        ChartFormat::Rpe
                    } else {
                        ChartFormat::Pgr
                    }
                } else {
                    ChartFormat::Pec
                }
            } else {
                ChartFormat::Pbc
            }
        });
        let mut chart = match format {
            ChartFormat::Rpe => parse_rpe(&String::from_utf8_lossy(&bytes), fs, extra, info.use_rpe_170_speed.unwrap_or_default()).await,
            ChartFormat::Pgr => parse_phigros(&String::from_utf8_lossy(&bytes), extra),
            ChartFormat::Pec => parse_pec(&String::from_utf8_lossy(&bytes), extra),
            ChartFormat::Pbc => {
                let mut r = BinaryReader::new(Cursor::new(&bytes));
                r.read()
            }
        }?;
        chart.load_textures(fs).await?;
        chart.settings.hold_partial_cover = info.hold_partial_cover;
        Ok((chart, bytes, format))
    }

    pub async fn new(
        mode: GameMode,
        info: ChartInfo,
        mut config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        background: SafeTexture,
        illustration: SafeTexture,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
        save_fn: Option<SaveFn>,
    ) -> Result<Self> {
        match mode {
            GameMode::TweakOffset => {
                config.mods.insert(Mods::AUTOPLAY);
            }
            GameMode::Exercise => {
                config.mods.remove(Mods::AUTOPLAY);
            }
            _ => {}
        }

        let (mut chart, chart_bytes, chart_format) = Self::load_chart(fs.deref_mut(), &info).await?;
        let effects = std::mem::take(&mut chart.extra.global_effects);
        if config.fxaa {
            chart
                .extra
                .effects
                .push(Effect::new(0.0..f32::INFINITY, include_str!("fxaa.glsl"), Vec::new(), false).unwrap());
        }

        let info_offset = info.offset;
        let mut res = Resource::new(
            config,
            info,
            fs,
            player.as_ref().and_then(|it| it.avatar.clone()),
            background,
            illustration,
            chart.extra.effects.is_empty() && effects.is_empty(),
        )
        .await
        .context("Failed to load resources")?;

        chart.hitsounds.drain().for_each(|(name, clip)| {
            if let Ok(clip) = res.create_sfx(clip) {
                res.extra_sfxs.insert(name, clip);
            }
        });

        let exercise_range = (chart.offset + info_offset + res.config.offset)..res.track_length;
        let hp_enabled = res.config.hp_bar_enabled;

        let judge = Judge::new(&chart);
        let hp_n = judge.result().num_of_notes;

        let music = Self::new_music(&mut res)?;
        Ok(Self {
            should_exit: false,
            next_scene: None,
            perfect_fade_start: None,
            perfect_flash_start: None,
            pure_memory_start: None,
           perfect_no_miss_start: None,

            mode,
            res,
            chart,
            judge,
            gl: unsafe { get_internal_gl() },
            player,
            chart_bytes,
            chart_format,
            effects,
            info_offset,
            display_score: 0,
score_anim_from: 0,
score_anim_to: 0,
score_anim_start: 0.0,
score_anim_duration: 0.50,
            first_in: false,
            exercise_range,
            exercise_press: None,
            exercise_btns: (RectButton::new(), RectButton::new()),

            music,

            state: State::Starting,
            last_update_time: 0.,
            pause_rewind: None,
            pause_first_time: f32::NEG_INFINITY,
            pause_disabled: false,
            highlight_start: f32::NEG_INFINITY,

            bad_notes: Vec::new(),

            upload_fn,
            update_fn,
save_fn,

            best_record: None,

            touch_points: Vec::new(),

            fps_frame_count: 0,
            fps_total_time: 0.0,
            fps_last_frame_time: 0.0,

            dead: false,

            hp_enabled,
            hp_value: 100.0,
            hp_rate_offset: 0.0,
            hp_last_counts: [0; 5],
            hp_max_score: 0,
            hp_dead: false,
            hp_red_blend: 0.0,
            hp_black_blend: 0.0,
            hp_prev_value: 100.0,
            hp_n,
            hp_consec_miss: 0,
            hp_doom_drain: false,

            skip_hold_start: None,
            skip_by_touch: false,
            skip_anim_start: None,
        })
    }

    pub fn set_pause_disabled(&mut self, disabled: bool) {
        self.pause_disabled = disabled;
    }

    pub fn get_avg_fps(&self) -> Option<f32> {
        if self.fps_frame_count > 0 && self.fps_total_time > 0.0 {
            Some(self.fps_frame_count as f32 / self.fps_total_time as f32)
        } else {
            None
        }
    }
    fn new_music(res: &mut Resource) -> Result<Music> {
        res.audio.create_music(
            res.music.clone(),
            MusicParams {
                amplifier: res.config.volume_music as _,
                playback_rate: res.config.speed as _,
                ..Default::default()
            },
        )
    }

    fn touch_scale(&self) -> f32 {
        (screen_width() / screen_height()) / self.res.aspect_ratio
    }

    fn ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now() as f32;
        let p = match self.state {
            State::Starting => {
                if time <= Self::BEFORE_TIME {
                    1. - (1. - time / Self::BEFORE_TIME).powi(3)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Playing => 1.,
            State::Ending => {
if self.perfect_flash_start.is_none() {
    let counts = self.judge.counts();
    let result = self.judge.result();

    if counts[0] + counts[1] == result.num_of_notes {

        self.perfect_flash_start = Some(tm.now() as f32);
                self.pure_memory_start = Some(tm.now() as f32);

    }
}
if self.perfect_no_miss_start.is_none() {
    let counts = self.judge.counts();
        let result = self.judge.result();

    if counts[2] != 0 && counts[0] + counts[1] + counts[2] == result.num_of_notes {

        self.perfect_no_miss_start = Some(tm.now() as f32);
    }
}

                let t = time - self.res.track_length - WAIT_TIME;
                1. - (t / (AFTER_TIME + 0.3)).min(1.).powi(2)
            }
        };
        let res = &mut self.res;
        let eps = 2e-2 / res.aspect_ratio;
        let top = -1. / res.aspect_ratio;
        let pause_w = 0.015;
        let pause_h = pause_w * 3.2;
        let counts = self.judge.counts();
        let pause_center = Point::new(pause_w * 4.0 - 1., top + eps * 3.5 - (1. - p) * 0.4 + pause_h / 2.);
        if res.config.interactive
            && !self.pause_disabled
            && !tm.paused()
            && self.pause_rewind.is_none()
            && Judge::get_touches().iter().any(|touch| {
                touch.phase == TouchPhase::Started && {
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    (pause_center - p).norm() < 0.05
                }
            })
        {
            let t = tm.now() as f32;
            if t - self.pause_first_time > PAUSE_CLICK_INTERVAL && res.config.double_click_to_pause {
                self.pause_first_time = t;
            } else {
                self.pause_first_time = f32::NEG_INFINITY;
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
            }
        }

        let real_now_f = tm.real_time() as f32;
        let enter_elapsed = (real_now_f - self.highlight_start).max(0.);
        let enter_p = (enter_elapsed / HUD_ENTER_DURATION).clamp(0., 1.);
        let enter_p_eased = 1. - (1. - enter_p).powi(3);
        let enter_dx = HUD_SLIDE_DIST_X * (1. - enter_p_eased);
        let exit_dx = (1. - p) * HUD_SLIDE_DIST_X;
        let exit_dy = -(1. - p) * HUD_SLIDE_DIST_Y;
        let hud_dx = enter_dx + exit_dx;
        let highlight_visible = enter_elapsed <= HUD_HIGHLIGHT_DURATION;

        ui.alpha(res.alpha, |ui| {
            ui.text("MAGIC BUGFIX TEXT").color(Color::new(0., 0., 0., 0.)).draw();

            let margin = 0.03;

            let unit_h = ui.text("0").measure_using(&PGR_FONT).h;

            let margin = 0.05;
            let base_y = top + margin;

            let score_y = base_y + 1.22;
            let result = self.judge.result();
            let counts = self.judge.counts();
let is_ap_plus =
    self.display_score == 10_000_000 + result.num_of_notes;
let is_fr = counts[2] != 0 && counts[3] + counts[4] == 0 && counts[0] + counts[1] + counts[2] == result.num_of_notes;
            let score = format!("{:08}", self.display_score);

let score_y = top + margin;
let score_y_anim = score_y + exit_dy;
let score_text = format!("{:08}", self.display_score);

let result = self.judge.result();
let is_ap_plus =
    self.display_score == 10_000_000 + result.num_of_notes;

let center_x: f32 = 0.004;
let right_x = 1.0 - margin;
self.chart.with_element(ui, res, UIElement::Score, Some((center_x, score_y_anim)), Some((center_x, score_y_anim)), |ui, c| {
    if is_ap_plus {
        let shadow_offset = 0.006;
        ui.text(&score_text)
            .pos(center_x, score_y_anim + shadow_offset)
            .anchor(0.5, 0.0)
            .size(0.9)
            .color(Color::new(0.25 * c.r, 0.55 * c.g, 1.0 * c.b, 0.9 * c.a))
            .draw_using(&PGR_FONT);
    }
    if is_fr {
        let shadow_offset = 0.006;
        ui.text(&score_text)
            .pos(center_x, score_y_anim + shadow_offset)
            .anchor(0.5, 0.0)
            .size(0.9)
            .color(Color::new(0.75 * c.r, 0.45 * c.g, 1.0 * c.b, 0.9 * c.a))
            .draw_using(&PGR_FONT);
    }
});

self.chart.with_element(ui, res, UIElement::Score, Some((right_x, score_y_anim)), Some((right_x, score_y_anim)), |ui, c| {
    if is_ap_plus {
        let shadow_offset = 0.006;
        ui.text(&score_text)
            .pos(right_x + 0.004, score_y_anim + shadow_offset)
            .anchor(1.0, 0.0)
            .size(0.9)
            .color(Color::new(0.25 * c.r, 0.55 * c.g, 1.0 * c.b, 0.9 * c.a))
            .draw_using(&PGR_FONT);
    }
    if is_fr {
        let shadow_offset = 0.006;
        ui.text(&score_text)
            .pos(right_x + 0.004, score_y_anim + shadow_offset)
            .anchor(1.0, 0.0)
            .size(0.9)
            .color(Color::new(0.75 * c.r, 0.45 * c.g, 1.0 * c.b, 0.9 * c.a))
            .draw_using(&PGR_FONT);
    }

    ui.text(&score_text)
        .pos(right_x, score_y_anim)
        .anchor(1.0, 0.0)
        .size(0.9)
        .color(c)
        .draw_using(&PGR_FONT);
});

if res.config.show_acc {
    let acc_str = format!("{:.2}%", self.judge.real_time_accuracy() * 100.0);
    self.chart.with_element(ui, res, UIElement::Score, Some((right_x + hud_dx, score_y + 0.09)), Some((right_x + hud_dx, score_y + 0.09)), |ui, c| {
        ui.text(&acc_str)
            .pos(right_x + hud_dx, score_y + 0.09)
            .anchor(1.0, 0.0)
            .size(0.4)
            .color(Color { a: c.a * 0.7, ..c })
            .draw_using(&PGR_FONT);
    });
}

            let pause_x = -1.0 + margin;
            let pause_y = top + margin;
            let pause_disabled = self.pause_disabled;
            self.chart.with_element(
                ui,
                res,
                UIElement::Pause,
                Some((pause_x, pause_y)),
                Some((pause_x, pause_y)),
                |ui, c| {
                    let w = 0.015;
                    let h = w * 3.0;

                    let icon = if pause_disabled {
                        Color::new(0.45, 0.45, 0.45, c.a * 0.55)
                    } else {
                        c
                    };
                    ui.fill_rect(Rect::new(pause_x, pause_y, w, h), icon);
                    ui.fill_rect(Rect::new(pause_x + w * 2.0, pause_y, w, h), icon);
                },
            );

            let right_x = 1.0 - margin;
            let base_y_o = top + margin;
            let base_y = base_y_o + 0.22;
            let info_name = res.info.name.clone();
            let info_composer = res.info.composer.clone();
            let info_level = res.info.level.clone();
            self.chart.with_element(ui, res, UIElement::Name, Some((right_x + hud_dx, base_y)), Some((right_x + hud_dx, base_y)), |ui, c| {
                ui.text(&info_name)
                    .pos(right_x + hud_dx, base_y)
                    .anchor(1.0, 0.0)
                    .size(0.7)
                    .max_width(0.7)
                    .color(c)
                    .draw();
            });
            self.chart.with_element(ui, res, UIElement::Name, Some((right_x + hud_dx, base_y + 0.07)), Some((right_x + hud_dx, base_y + 0.07)), |ui, c| {
                ui.text(&info_composer)
                    .pos(right_x + hud_dx, base_y + 0.07)
                    .anchor(1.0, 0.0)
                    .size(0.5)
                    .color(Color { a: c.a * 0.7, ..c })
                    .draw();
            });
            self.chart.with_element(ui, res, UIElement::Level, Some((right_x + hud_dx, base_y + 0.12)), Some((right_x + hud_dx, base_y + 0.12)), |ui, c| {
                ui.text(&info_level)
                    .pos(right_x + hud_dx, base_y + 0.12)
                    .anchor(1.0, 0.0)
                    .size(0.5)
                    .color(Color { a: c.a * 0.7, ..c })
                    .draw();
            });

            let combo_value = self.judge.combo();
            if combo_value > 0 {
                let combo_text = combo_value.to_string();
                self.chart.with_element(ui, res, UIElement::ComboNumber, Some((0.0, 0.0)), Some((0.0, 0.0)), |ui, c| {

                    ui.text(&combo_text)
                        .pos(0.0, 0.0)
                        .anchor(0.5, 0.5)
                        .size(2.2)
                        .color(Color::new(0.6 * c.r, 0.6 * c.g, 0.6 * c.b, 0.5 * c.a))
                        .draw_using(&PGR_FONT);
                });
            }

            let hw = 0.003_f32;
            let height = eps;
            let offset = self.chart.offset + self.info_offset + res.config.offset;
            let span = (self.exercise_range.end - self.exercise_range.start).max(1e-3);
            let dest = ((res.time as f32 - self.exercise_range.start + offset) / span).clamp(0., 1.);
            let dest_w = 2. * dest;
            self.chart.with_element(ui, res, UIElement::Bar, Some((-1., top + height / 2.)), Some((-1., top + height / 2.)), |ui, _| {
                ui.fill_rect(Rect::new(-1., top, dest_w, height), semi_white(0.6));
                ui.fill_rect(Rect::new(-1. + dest_w - hw, top, hw * 2., height), WHITE);
            });

            {
                let counts = self.judge.counts();
                let right_x = 1.0 - margin;
                let center_y = 0.0;
                let line_h = 0.065;
                let start_y = center_y - line_h * 2.0;
                let size = 0.34;
                let items = [
                    ("PERFECT+", counts[1], Color::new(1.00, 0.90, 0.35, 1.0)),
                    ("PERFECT",  counts[0], Color::new(1.00, 0.78, 0.20, 1.0)),
                    ("GOOD",     counts[2], Color::new(0.20, 0.85, 0.75, 1.0)),
                    ("BAD",      counts[3], Color::new(0.95, 0.25, 0.25, 1.0)),
                    ("MISS",     counts[4], Color::new(0.65, 0.65, 0.65, 1.0)),
                ];
                let panel_x = right_x + hud_dx;
                self.chart.with_element(ui, res, UIElement::Combo, Some((panel_x, center_y)), Some((panel_x, center_y)), |ui, c| {
                    for (i, (label, count, color)) in items.iter().enumerate() {
                        let tint = Color::new(color.r * c.r, color.g * c.g, color.b * c.b, color.a * c.a);
                        ui.text(format!("{label}  {count}"))
                            .pos(panel_x, start_y + i as f32 * line_h)
                            .anchor(1.0, 0.0)
                            .size(size)
                            .color(tint)
                            .draw_using(&PGR_FONT);
                    }
                });
            }

        });

        if self.res.config.watermark_enabled {
    let c = self.res.config.watermark_color;

    let bottom_margin = 0.04;
    let bottom = -top;
    ui.text(&self.res.config.watermark_text)
        .pos(0.0, bottom - bottom_margin)
        .anchor(0.5, 1.0)
        .size(self.res.config.watermark_size)
        .color(Color::new(c[0], c[1], c[2], c[3]))
        .draw();
}

        Ok(())
    }

    fn update_hp(&mut self, tm: &mut TimeManager, dt: f32) -> bool {
        if !self.hp_enabled || self.hp_dead { return false; }
        let playing = matches!(self.state, State::Playing);

        if playing {
            let c = self.judge.counts();
            let d_pure = (c[0] + c[1]).saturating_sub(self.hp_last_counts[0] + self.hp_last_counts[1]) as u32;
            let d_far  = c[2].saturating_sub(self.hp_last_counts[2]) as u32;
            let d_bad  = c[3].saturating_sub(self.hp_last_counts[3]) as u32;
            let d_miss = c[4].saturating_sub(self.hp_last_counts[4]) as u32;
            let d_lost = d_bad + d_miss;
            let inv_tau = 1.0 / HP_IMPULSE_TAU;
            self.hp_rate_offset += d_pure as f32 * HP_PERFECT * inv_tau;
            self.hp_rate_offset += d_far  as f32 * HP_GOOD    * inv_tau;
            self.hp_rate_offset += d_lost as f32 * HP_MISS    * inv_tau;

            if d_pure + d_far > 0 {
                self.hp_consec_miss = 0;
            }
            self.hp_consec_miss = self.hp_consec_miss.saturating_add(d_lost);
            if self.hp_consec_miss > HP_LOCK_STREAK {
                self.hp_doom_drain = true;
            }

            self.hp_last_counts = c;
        }

        let rate = if self.hp_doom_drain {
            HP_DOOM_RATE
        } else {
            let v_raw = -(self.hp_n as f32) * HP_L + self.hp_rate_offset;
            if v_raw > HP_GAIN_CAP { HP_GAIN_CAP } else { v_raw }
        };
        let prev = self.hp_value;
        self.hp_value = (self.hp_value + rate * dt).clamp(0., 100.);

        let decay = (-dt / HP_IMPULSE_TAU).exp();
        self.hp_rate_offset *= decay;

        let going_down = self.hp_value + 1e-5 < self.hp_prev_value;
        self.hp_prev_value = self.hp_value;
        let red_target = if going_down { 1.0 } else { 0.0 };
        let red_k = (1.8 * dt).min(1.0);
        self.hp_red_blend += (red_target - self.hp_red_blend) * red_k;
        let black_target = ((25.0 - self.hp_value) / 25.0).clamp(0., 1.);
        let black_k = (2.0 * dt).min(1.0);
        self.hp_black_blend += (black_target - self.hp_black_blend) * black_k;

        let s = self.judge.score();
        if s > self.hp_max_score { self.hp_max_score = s; }

        if playing && self.hp_value <= 0.0 && !self.hp_dead {
            self.hp_dead = true;

            let _ = self.music.pause();
            if !tm.paused() {   }
            self.state = State::Ending;

            let now = tm.now() as f32;
            let want = self.res.track_length + WAIT_TIME;
            if now < want {
                tm.seek_to(want as f64);
            }

            self.perfect_flash_start = Some(now);
            self.perfect_no_miss_start = Some(now);
            return true;
        }
        let _ = prev;
        false
    }

    fn ui_classic(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now() as f32;
        let p = match self.state {
            State::Starting => {
                if time <= Self::BEFORE_TIME {
                    1. - (1. - time / Self::BEFORE_TIME).powi(3)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Playing => 1.,
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME as f32;
                1. - (t / AFTER_TIME as f32).min(1.).powi(2)
            }
        };

        let cs = crate::custom_style::current();
        let res = &mut self.res;
        let c = Color::new(1., 1., 1., res.alpha);
        let aspect_ratio = res.aspect_ratio;
        let top: f32 = -1. / aspect_ratio;
        let eps: f32 = 2e-2 / aspect_ratio;
        let margin = 0.0425;
        let pause_w = 0.013;
        let pause_h = pause_w * 3.2;
        let pause_center = Point::new(
            -1. + 0.0525,
            top + eps * 3.6454 - (1. - p) * 0.4 + pause_h / 2.,
        );

        if res.config.interactive
            && !self.pause_disabled
            && !tm.paused()
            && self.pause_rewind.is_none()
            && matches!(self.state, State::Playing)
            && Judge::get_touches().iter().any(|touch| {
                touch.phase == TouchPhase::Started && {
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    (pause_center - p).norm() < 0.05
                }
            })
        {
            let t = tm.now() as f32;
            if t - self.pause_first_time > PAUSE_CLICK_INTERVAL && res.config.double_click_to_pause {
                self.pause_first_time = t;
            } else {
                self.pause_first_time = f32::NEG_INFINITY;
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
            }
        }
        if tm.now() as f32 - self.pause_first_time <= PAUSE_CLICK_INTERVAL {
            ui.fill_circle(pause_center.x, pause_center.y, 0.05, Color::new(1., 1., 1., 0.5));
        }

        let score = self.display_score;
        let score_str = format!("{:08}", score);
        let score_top = top + eps * 2.8125 - (1. - p) * 0.4;
        let score_right = 1. - margin + 0.001;
        ui.text("AA").color(Color::new(0., 0., 0., 0.)).draw();
        let text_size = 0.9;
        let score_style = &cs.score;
        if score_style.visible(true) {
            let (sx, sy) = score_style.pos(score_right, score_top);
            let (sax, say) = score_style.anchor(1., 0.);
            let ssz = score_style.size(text_size);
            let sfont = score_style.font(crate::custom_style::StyleFont::Pgr);
            self.chart.with_element(
                ui,
                res,
                UIElement::Score,
                Some((sx, sy)),
                Some((sx, sy)),
                |ui, color| {
                    let base = Color { a: color.a * c.a, ..color };
                    let final_c = score_style.color(base);
                    let mut t = ui.text(&score_str).pos(sx, sy).anchor(sax, say).size(ssz).color(final_c);
                    match sfont {
                        crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                        crate::custom_style::StyleFont::Default => { t.draw(); }
                    }
                },
            );
        }

        let acc_style = &cs.accuracy;
        if res.config.show_acc && acc_style.visible(true) {
            let default_x = 1. - margin;
            let default_y = score_top + 0.095;
            let (ax, ay) = acc_style.pos(default_x, default_y);
            let (aax, aay) = acc_style.anchor(1., 0.);
            let asz = acc_style.size(0.4);
            let afont = acc_style.font(crate::custom_style::StyleFont::Default);
            let base = Color { a: c.a * 0.7, ..c };
            let acolor = acc_style.color(base);
            let txt = format!("{:05.2}%", self.judge.real_time_accuracy() * 100.);
            let mut t = ui.text(txt).pos(ax, ay).anchor(aax, aay).size(asz).color(acolor);
            match afont {
                crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                crate::custom_style::StyleFont::Default => { t.draw(); }
            }
        }

        self.chart.with_element(
            ui,
            res,
            UIElement::Pause,
            Some((pause_center.x - pause_w * 1.5, pause_center.y - pause_h * 0.5)),
            Some((pause_center.x - pause_w * 1.5, pause_center.y - pause_h * 0.5)),
            |ui, color| {
                let pc = Color { a: color.a * c.a, ..color };
                let mut r = Rect::new(
                    pause_center.x - pause_w / 2.,
                    pause_center.y - pause_h / 2.,
                    pause_w,
                    pause_h,
                );
                r.x -= pause_w;
                ui.fill_rect(r, pc);
                r.x += pause_w * 2.;
                ui.fill_rect(r, pc);
            },
        );

        if self.judge.combo() >= 3 {
            let combo = self.judge.combo().to_string();
            let combo_size_default = 0.98;
            let cn_style = &cs.combo_number;
            let combo_label_style = &cs.combo;
            let cn_size = cn_style.size(combo_size_default);
            let mut probe = ui.text(&combo).size(cn_size).color(Color::new(0., 0., 0., 0.));
            let ct = probe.measure().center();
            let default_combo_y = top + eps * 1.55 - (1. - p) * 0.4 + ct.y;
            let (cnx, cny) = cn_style.pos(0., default_combo_y);
            let (cnax, cnay) = cn_style.anchor(0.5, 0.5);
            let cn_font = cn_style.font(crate::custom_style::StyleFont::Pgr);
            let btm = probe.anchor(0.5, 0.5).pos(0., default_combo_y).draw().bottom() + 0.015;

            if cn_style.visible(true) {
                self.chart.with_element(
                    ui,
                    res,
                    UIElement::ComboNumber,
                    Some((cnx, cny)),
                    Some((cnx, cny)),
                    |ui, color| {
                        let base = Color { a: color.a * c.a, ..color };
                        let final_c = cn_style.color(base);
                        let mut t = ui.text(&combo).pos(cnx, cny).anchor(cnax, cnay).size(cn_size).color(final_c);
                        match cn_font {
                            crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                            crate::custom_style::StyleFont::Default => { t.draw(); }
                        }
                    },
                );
            }

            if combo_label_style.visible(true) {
                let combo_label = res.config.combotext.clone();
                let cl_size = combo_label_style.size(0.34);
                let (clx, cly) = combo_label_style.pos(0., btm);
                let (clax, clay) = combo_label_style.anchor(0.5, 0.);
                let cl_font = combo_label_style.font(crate::custom_style::StyleFont::Default);
                self.chart.with_element(
                    ui,
                    res,
                    UIElement::Combo,
                    Some((clx, cly)),
                    Some((clx, cly)),
                    |ui, color| {
                        let base = Color { a: color.a * c.a, ..color };
                        let final_c = combo_label_style.color(base);
                        let mut t = ui.text(&combo_label).pos(clx, cly).anchor(clax, clay).size(cl_size).color(final_c);
                        match cl_font {
                            crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                            crate::custom_style::StyleFont::Default => { t.draw(); }
                        }
                    },
                );
            }
        }

        let lf = -1. + margin;
        let bt = -top - eps * 3.5 + (1. - p) * 0.4;

        let name_style = &cs.name;
        if name_style.visible(true) {
            let name_str = res.info.name.clone();
            let (nx, ny) = name_style.pos(lf, bt);
            let (nax, nay) = name_style.anchor(0., 1.);
            let nsz = name_style.size(0.505);
            let nfont = name_style.font(crate::custom_style::StyleFont::Default);
            self.chart.with_element(
                ui,
                res,
                UIElement::Name,
                Some((nx, ny)),
                Some((nx, ny)),
                |ui, color| {
                    let base = Color { a: color.a * c.a, ..color };
                    let final_c = name_style.color(base);
                    let mut t = ui.text(&name_str).pos(nx, ny).anchor(nax, nay).size(nsz).color(final_c);
                    match nfont {
                        crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                        crate::custom_style::StyleFont::Default => { t.draw(); }
                    }
                },
            );
        }

        let level_style = &cs.level;
        if level_style.visible(true) {
            let level_str = res.info.level.clone();
            let (lx, ly) = level_style.pos(-lf, bt);
            let (lax, lay) = level_style.anchor(1., 1.);
            let lsz = level_style.size(0.505);
            let lfont = level_style.font(crate::custom_style::StyleFont::Default);
            self.chart.with_element(
                ui,
                res,
                UIElement::Level,
                Some((lx, ly)),
                Some((lx, ly)),
                |ui, color| {
                    let base = Color { a: color.a * c.a, ..color };
                    let final_c = level_style.color(base);
                    let mut t = ui.text(&level_str).pos(lx, ly).anchor(lax, lay).size(lsz).color(final_c);
                    match lfont {
                        crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                        crate::custom_style::StyleFont::Default => { t.draw(); }
                    }
                },
            );
        }

        let wm_style = &cs.watermark;
        if res.config.watermark_enabled && !res.config.watermark_text.is_empty() && wm_style.visible(true) {
            let wc_arr = res.config.watermark_color;
            let base_wc = Color::new(wc_arr[0], wc_arr[1], wc_arr[2], wc_arr[3] * c.a);
            let final_wc = wm_style.color(base_wc);
            let default_wx = 0.;
            let default_wy = -top * 0.98 + (1. - p) * 0.4;
            let (wx, wy) = wm_style.pos(default_wx, default_wy);
            let (wax, way) = wm_style.anchor(0.5, 1.);
            let wsz = wm_style.size(res.config.watermark_size);
            let wfont = wm_style.font(crate::custom_style::StyleFont::Default);
            let mut t = ui.text(&res.config.watermark_text).pos(wx, wy).anchor(wax, way).size(wsz).color(final_wc);
            match wfont {
                crate::custom_style::StyleFont::Pgr => { t.draw_using(&PGR_FONT); }
                crate::custom_style::StyleFont::Default => { t.draw(); }
            }
        }

        let hw = 0.003_f32;
        let height = eps;
        let offset = self.chart.offset + self.info_offset + res.config.offset;
        let span = (self.exercise_range.end - self.exercise_range.start).max(1e-3);
        let dest = ((res.time as f32 - self.exercise_range.start + offset) / span).clamp(0., 1.);
        let dest_w = 2. * dest;
        let bar_style = &cs.bar;
        if bar_style.visible(true) {
            self.chart.with_element(
                ui,
                res,
                UIElement::Bar,
                Some((-1., top + height / 2.)),
                Some((-1., top + height / 2.)),
                |ui, color| {
                    let base = Color { a: color.a * c.a, ..color };
                    let final_c = bar_style.color(base);
                    ui.fill_rect(Rect::new(-1., top, dest_w, height), final_c);
                    ui.fill_rect(
                        Rect::new(-1. + dest_w - hw, top, hw * 2., height),
                        Color::new(0.95, 0.95, 0.95, color.a * c.a),
                    );
                },
            );
        }

        if self.hp_enabled {
            self.render_hp_bar(ui);
        }

        Ok(())
    }

    fn render_hp_bar(&self, ui: &mut Ui) {
        if !self.hp_enabled { return; }
        let h = 1.0 / self.res.aspect_ratio;

        let bar_w = 0.026;
        let bar_h = h * 1.5;
        let bar_x = -1.0 + 0.045;
        let bar_top = -bar_h * 0.5;
        let bar_bottom = bar_top + bar_h;
        let border_t = 0.003;

        ui.fill_rect(
            Rect::new(bar_x - border_t, bar_top - border_t, bar_w + border_t * 2.0, bar_h + border_t * 2.0),
            Color::new(0.85, 0.85, 0.85, 0.55),
        );
        ui.fill_rect(
            Rect::new(bar_x, bar_top, bar_w, bar_h),
            Color::new(0.06, 0.06, 0.06, 0.85),
        );

        let ratio = (self.hp_value / 100.0).clamp(0., 1.);
        let fill_h = bar_h * ratio;
        let fill_y = bar_bottom - fill_h;

        let blue  = (0.25, 0.55, 1.00);
        let red   = (1.00, 0.22, 0.22);
        let black = (0.04, 0.04, 0.04);
        let rb = self.hp_red_blend.clamp(0., 1.);
        let kb = self.hp_black_blend.clamp(0., 1.);
        let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let r0 = mix(blue.0, red.0, rb);
        let g0 = mix(blue.1, red.1, rb);
        let b0 = mix(blue.2, red.2, rb);
        let r = mix(r0, black.0, kb);
        let g = mix(g0, black.1, kb);
        let bl = mix(b0, black.2, kb);
        ui.fill_rect(Rect::new(bar_x, fill_y, bar_w, fill_h), Color::new(r, g, bl, 1.0));

        ui.text(format!("{:.0}", self.hp_value.max(0.0)))
            .pos(bar_x + bar_w * 0.5, bar_bottom + 0.018)
            .anchor(0.5, 0.0)
            .no_baseline()
            .size(0.55)
            .color(WHITE)
            .draw();
    }

    fn overlay_ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let c = semi_white(self.res.alpha);
        let res = &mut self.res;

        if tm.paused() && self.skip_anim_start.is_none() {
            let h = 1. / res.aspect_ratio;
            let _ = c;

            draw_rectangle(-1., -h, 2., h * 2., Color::new(0.05, 0.02, 0.06, 0.72));
            let o = if self.mode == GameMode::Exercise { -0.28 } else { 0. };
            let no_retry = self.mode == GameMode::NoRetry;
            let a = res.alpha;
            let pink = Color::new(1.0, 0.58, 0.706, a);
            let cream = Color::new(0.984, 0.973, 0.886, a);

            let card = Rect::new(-0.34, o - 0.25, 0.68, 0.50);
            ui.fill_path(&card.feather(0.016).rounded(0.07), Color::new(0.949, 0.412, 0.580, 0.40 * a));
            ui.fill_path(&card.rounded(0.06), Color::new(0.165, 0.110, 0.180, 0.98 * a));
            ui.text("PAUSED")
                .pos(0., card.y + 0.072)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.92)
                .color(cream)
                .draw();
            ui.fill_path(&Rect::new(-0.06, card.y + 0.118, 0.12, 0.006).rounded(0.003), pink);

            let btn_x = card.x + 0.05;
            let btn_w = card.w - 0.10;
            let btn_h = 0.105;
            let gap = 0.022;
            let btn_top = card.y + 0.16;
            let row_rect = |row: i32| Rect::new(btn_x, btn_top + row as f32 * (btn_h + gap), btn_w, btn_h);
            let draw_pill = |ui: &mut Ui, br: Rect, tex: &SafeTexture, label: &str, primary: bool, enabled: bool| {
                let ea = if enabled { 1.0 } else { 0.4 };
                if primary && enabled {
                    ui.fill_path(&br.rounded(0.05), Color::new(0.949, 0.412, 0.580, a));
                } else {
                    ui.fill_path(&br.rounded(0.05), Color::new(0.243, 0.165, 0.255, 0.96 * a * ea));
                }
                let isz = br.h * 0.56;
                let ir = Rect::new(br.x + 0.045, br.center().y - isz * 0.5, isz, isz);
                let icon_c = if primary && enabled { cream } else { Color::new(pink.r, pink.g, pink.b, pink.a * ea) };
                ui.fill_rect(ir, (**tex, ir, ScaleType::Fit, icon_c));
                ui.text(label)
                    .pos(ir.right() + 0.035, br.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.56)
                    .color(Color::new(cream.r, cream.g, cream.b, cream.a * ea))
                    .draw();
            };
            draw_pill(ui, row_rect(0), &res.icon_resume, "Resume", true, true);
            draw_pill(ui, row_rect(1), &res.icon_retry, "Retry", false, !no_retry);
            draw_pill(ui, row_rect(2), &res.icon_back, "Exit", false, true);
            if res.config.interactive {
                let mut clicked = None;
                for touch in Judge::get_touches() {
                    if touch.phase != TouchPhase::Started {
                        continue;
                    }
                    let p = touch.position;
                    if row_rect(0).contains(p) {
                        clicked = Some(1);
                    } else if row_rect(1).contains(p) {
                        clicked = Some(0);
                    } else if row_rect(2).contains(p) {
                        clicked = Some(-1);
                    }
                }
                if no_retry && clicked == Some(0) {
                    clicked = None;
                }
                let mut pos = self.music.position();
                if self.mode == GameMode::Exercise {
                    pos = tm.now() as f32;
                }
                if clicked.map_or(false, |it| it != -1) && (tm.speed - res.config.speed as f64).abs() > 0.01 {
                    debug!("recreating music");
                    self.music = res.audio.create_music(
                        res.music.clone(),
                        MusicParams {
                            amplifier: res.config.volume_music as _,
                            playback_rate: res.config.speed as _,
                            ..Default::default()
                        },
                    )?;
                }
                match clicked {
                    Some(-1) => {
                        self.should_exit = true;
                    }
                    Some(0) => {
                        reset!(self, res, tm);
                    }
                    Some(1) => {
                        if self.mode == GameMode::Exercise && (tm.now() > self.exercise_range.end as f64 || tm.now() < self.exercise_range.start as f64) {
                            tm.seek_to(self.exercise_range.start as f64);
                            self.music.seek_to(self.exercise_range.start)?;
                            pos = self.exercise_range.start;
                        }
                        self.music.play()?;
                        res.time -= 3.;
                        let dst = pos - 3.;
                        if dst < 0. {
                            self.music.pause()?;
                            self.state = State::BeforeMusic;
                        } else {
                            self.music.seek_to(dst)?;
                        }
                        let now = tm.now() ;
                        tm.speed = res.config.speed as _;
                        tm.resume();
                        tm.seek_to(now - 3.);
                        self.pause_rewind = Some(tm.now() - 0.2);
                    }
                    _ => {}
                }
            }
            if self.mode == GameMode::Exercise {
                let asp = self.touch_scale();
                for touch in ui.ensure_touches() {
                    touch.position *= asp;
                }

                ui.dy(h * 0.50);
                ui.scope(|ui| {
                    ui.dx(0.3);
                    ui.dy(-0.3);
                    ui.slider(tl!("speed"), 0.5..2.0, 0.05, &mut self.res.config.speed, Some(0.5));
                });
                ui.dy(0.06);
                let hw = 0.7;
                let h = 0.06;
                let eh = 0.12;
                let rad = 0.03;
                let sp = self.offset().min(0.);

                let track_c = Color::new(0.165, 0.110, 0.180, 0.92);
                let range_c = Color::new(1.0, 0.58, 0.706, 0.85);
                let start_c = Color::new(0.682, 0.804, 0.780, 1.0);
                let end_c = Color::new(0.992, 0.847, 0.694, 1.0);
                let cur_c = Color::new(0.984, 0.973, 0.886, 1.0);
                ui.fill_path(&Rect::new(-hw, -h, hw * 2., h * 2.).rounded(h * 0.6), track_c);
                let st = -hw + (self.exercise_range.start - sp) / (self.res.track_length - sp) * hw * 2.;
                let en = -hw + (self.exercise_range.end - sp) / (self.res.track_length - sp) * hw * 2.;
                let t = tm.now() as f32;
                let cur = -hw + (t - sp) / (self.res.track_length - sp) * hw * 2.;
                ui.fill_rect(Rect::new(st, -h, en - st, h * 2.), range_c);
                ui.fill_rect(Rect::new(st, -eh, 0., eh + h).feather(0.005), start_c);
                ui.fill_circle(st, -eh, rad, start_c);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(st, -eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (-1, it.id));
                }
                ui.fill_rect(Rect::new(en, -h, 0., eh + h).feather(0.005), end_c);
                ui.fill_circle(en, eh, rad, end_c);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(en, eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (1, it.id));
                }
                ui.fill_rect(Rect::new(cur, -h, 0., h * 2.).feather(0.005), cur_c);
                ui.fill_circle(cur, 0., rad, cur_c);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(cur, 0., 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (0, it.id));
                }
                ui.text(fmt_time(t)).pos(0., -0.23).anchor(0.5, 0.).size(0.8).color(cur_c).draw();
                if let Some((ctrl, id)) = &self.exercise_press {
                    if let Some(touch) = Judge::get_touches().iter().rfind(|it| it.id == *id) {
                        let x = touch.position.x;
                        let p = (x + hw) / (hw * 2.) * (self.res.track_length - sp) + sp;
                        let p = if self.res.track_length - sp <= 3. || *ctrl == 0 {
                            p.clamp(sp, self.res.track_length)
                        } else {
                            p.clamp(
                                if *ctrl == -1 { sp } else { self.exercise_range.start + 3. },
                                if *ctrl == -1 {
                                    self.exercise_range.end - 3.
                                } else {
                                    self.res.track_length
                                },
                            )
                        };
                        if *ctrl == 0 {
                            tm.seek_to(p as f64);
                            self.music.seek_to(p)?;
                        } else {
                            *(if *ctrl == -1 {
                                &mut self.exercise_range.start
                            } else {
                                &mut self.exercise_range.end
                            }) = p;
                        }
                        if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                            self.exercise_press = None;
                        }
                    }
                }
                ui.dy(0.1);
                let r = ui.text(tl!("to")).size(0.8).anchor(0.5, 0.).draw();
                let mut tx = ui
                    .text(fmt_time(self.exercise_range.start))
                    .pos(r.x - 0.02, 0.)
                    .anchor(1., 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.0.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.0.touching() { 0.5 } else { 1. }));
                tx.draw();

                let mut tx = ui
                    .text(fmt_time(self.exercise_range.end))
                    .pos(r.right() + 0.02, 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.1.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.1.touching() { 0.5 } else { 1. }));
                tx.draw();
                for touch in ui.ensure_touches() {
                    touch.position /= asp;
                }
            }
        }
        if let Some(time) = self.pause_rewind {
            let dt = tm.now() - time;
            let t = 3 - dt.floor() as i32;
            if t <= 0 {
                self.pause_rewind = None;
            } else {
                let a = (1. - dt as f32 / 3.) * 1.;
                let h = 1. / self.res.aspect_ratio;
                draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., a));
                ui.text(t.to_string()).anchor(0.5, 0.5).size(1.).color(c).draw();
            }
        }
        if self.res.config.touch_debug {
            for touch in Judge::get_touches() {
                ui.fill_circle(touch.position.x, touch.position.y, 0.04, Color { a: 0.4, ..RED });
            }
        }
        for pos in &self.touch_points {
            ui.fill_circle(pos.0, pos.1, 0.04, Color { a: 0.4, ..BLUE });
        }
        Ok(())
    }

    fn interactive(res: &Resource, state: &State) -> bool {
        res.config.interactive && matches!(state, State::Playing)
    }

    fn offset(&self) -> f32 {
        self.chart.offset + self.res.config.offset + self.info_offset
    }

    fn tweak_offset(&mut self, ui: &mut Ui, ita: bool) {
        ui.scope(|ui| {
            let width = 0.55;
            let height = 0.4;
            ui.dx(1. - width - 0.02);
            ui.dy(ui.top - height - 0.02);
            ui.fill_rect(Rect::new(0., 0., width, height), GRAY);
            ui.dy(0.02);
            ui.text(tl!("adjust-offset")).pos(width / 2., 0.).anchor(0.5, 0.).size(0.7).draw();
            ui.dy(0.16);
            let r = ui
                .text(format!("{}ms", (self.info_offset * 1000.).round() as i32))
                .pos(width / 2., 0.)
                .anchor(0.5, 0.)
                .size(0.6)
                .no_baseline()
                .draw();
            let d = 0.14;
            if ui.button("lg_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.026), "-") && ita {
                self.info_offset -= 0.05;
            }
            if ui.button("lg_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.026), "+") && ita {
                self.info_offset += 0.05;
            }
            let d = 0.08;
            if ui.button("sm_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.022), "-") && ita {
                self.info_offset -= 0.005;
            }
            if ui.button("sm_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.022), "+") && ita {
                self.info_offset += 0.005;
            }
            let d = 0.03;
            if ui.button("ti_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.017), "-") && ita {
                self.info_offset -= 0.001;
            }
            if ui.button("ti_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.017), "+") && ita {
                self.info_offset += 0.001;
            }
            ui.dy(0.14);
            let pad = 0.02;
            let spacing = 0.01;
            let mut r = Rect::new(pad, 0., (width - pad * 2. - spacing * 2.) / 3., 0.06);
            if ui.button("cancel", r, tl!("offset-cancel")) {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(None::<f32>)));
            }
            r.x += r.w + spacing;
            if ui.button("reset", r, tl!("offset-reset")) {
                self.info_offset = 0.;
            }
            r.x += r.w + spacing;
            if ui.button("save", r, tl!("offset-save")) {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(self.info_offset))));
            }
        });
    }

    fn skip_allowed(&self) -> bool {
        matches!(self.mode, GameMode::Normal | GameMode::NoRetry | GameMode::View)
    }

    fn update_skip(&mut self, tm: &mut TimeManager) -> Result<()> {

        if let Some(start) = self.skip_anim_start {
            if self.next_scene.is_none() && tm.real_time() - start >= SKIP_ANIM_TIME {
                tm.resume();
                self.next_scene = Some(self.skip_ending()?);
            }
            return Ok(());
        }

        if !self.skip_allowed() || !matches!(self.state, State::Playing) || tm.paused() || self.pause_rewind.is_some() || !self.res.config.interactive {
            self.skip_hold_start = None;
            return Ok(());
        }
        let key_held = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);

        let top = -1. / self.res.aspect_ratio;
        let touch_held = Judge::get_touches()
            .iter()
            .any(|t| !matches!(t.phase, TouchPhase::Ended | TouchPhase::Cancelled) && t.position.x >= 0.2 && t.position.y <= top + 0.3);
        let now = tm.real_time();
        if key_held || touch_held {
            if self.skip_hold_start.is_none() {
                self.skip_hold_start = Some(now);
                self.skip_by_touch = touch_held && !key_held;
            }
            if now - self.skip_hold_start.unwrap() >= SKIP_HOLD_TIME {

                self.skip_hold_start = None;
                self.skip_anim_start = Some(now);
                let _ = self.music.pause();
                tm.pause();
            }
        } else {

            self.skip_hold_start = None;
        }
        Ok(())
    }

    fn skip_ending(&mut self) -> Result<NextScene> {
        Ok(NextScene::Overlay(Box::new(EndingScene::new(
            self.res.background.clone(),
            self.res.illustration.clone(),
            self.res.player.clone(),
            self.res.icons.clone(),
            self.res.icon_retry.clone(),
            self.res.icon_proceed.clone(),
            self.res.mod_icons.clone(),
            self.res.info.clone(),
            self.judge.result(),
            &self.res.config,
            self.res.res_pack.ending.clone(),
            self.upload_fn.as_ref().map(Arc::clone),
            self.player.as_ref().map(|it| it.rks),
            self.player.as_ref().map_or(0, |it| it.historic_best),
            None,
            self.best_record.clone(),
            if self.res.config.show_avg_fps { self.get_avg_fps() } else { None },
        )?)))
    }

    fn render_skip(&self, ui: &mut Ui, tm: &mut TimeManager) {
        let top = ui.top;
        let now = tm.real_time();

        if let (Some(start), None) = (self.skip_hold_start, self.skip_anim_start) {
            let held = ((now - start).max(0.)) as f32;
            let p = (held / SKIP_HOLD_TIME as f32).clamp(0., 1.);
            let remain = (SKIP_HOLD_TIME as f32 - held).max(0.).ceil().max(1.) as i32;
            let msg = if self.skip_by_touch {
                format!("Skipping track in {remain} seconds.")
            } else {
                format!("Skipping track in {remain} seconds. Release Alt button to cancel.")
            };
            let bar_h = 0.09;
            let bar = Rect::new(-1., -top, 2., bar_h);
            let fill = Rect::new(-1., -top, 2. * p, bar_h);
            let text_y = -top + bar_h / 2.;
            let size = 0.4;

            ui.fill_rect(bar, Color::new(0., 0., 0., 0.78));

            ui.text(&msg).pos(0., text_y).anchor(0.5, 0.5).no_baseline().size(size).color(WHITE).draw();

            ui.fill_rect(fill, WHITE);

            ui.scissor(fill, |ui| {
                ui.text(&msg).pos(0., text_y).anchor(0.5, 0.5).no_baseline().size(size).color(BLACK).draw();
            });
        }

        if let Some(start) = self.skip_anim_start {
            let t = ((now - start).max(0.)) as f32;
            let curtain = (t / 0.8).clamp(0., 1.);
            let eased = 1.0 - (1.0 - curtain).powi(3);
            ui.fill_rect(Rect::new(-1., -top, 2., eased * 2. * top), Color::new(0., 0., 0., 0.5));
            let fade = (t / 0.5).clamp(0., 1.);
            ui.text("TRACK SKIPPED")
                .pos(0., 0.)
                .anchor(0.5, 0.5)
                .size(1.2)
                .color(Color::new(1., 1., 1., fade))
                .draw_using(&PGR_FONT);
        }
    }
}

impl Scene for GameScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        on_game_start();
        self.music = Self::new_music(&mut self.res)?;
        self.res.camera.render_target = target;
        tm.speed = self.res.config.speed as _;
        tm.adjust_time = self.res.config.adjust_time;
        reset!(self, self.res, tm);
        set_camera(&self.res.camera);
        self.first_in = true;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        if !tm.paused() {
            self.pause_rewind = None;
            self.music.pause()?;
            tm.pause();
        }
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {

        if self.pause_disabled && matches!(self.state, State::Playing) {
            let _ = self.music.play();
            tm.resume();
            return Ok(());
        }
        if !matches!(self.state, State::Playing) {
            tm.resume();
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.res.audio.recover_if_needed()?;
        self.update_skip(tm)?;
        if matches!(self.state, State::Playing) {
            tm.update(self.music.position() as f64);
        }
        if self.mode == GameMode::Exercise && tm.now() > self.exercise_range.end as f64 && !tm.paused() {
            let state = self.state.clone();
            reset!(self, self.res, tm);
            self.state = state;
            tm.seek_to(self.exercise_range.start as f64);
            tm.pause();
            self.music.pause()?;
        }
        let real_score = self.judge.score();

if real_score != self.score_anim_to {
    self.score_anim_from = self.display_score;
    self.score_anim_to = real_score;
    self.score_anim_start = tm.real_time();
}
        let now = tm.real_time();
let elapsed = now - self.score_anim_start;

if elapsed >= self.score_anim_duration {
    self.display_score = self.score_anim_to;
} else {
    let total = (self.score_anim_to - self.score_anim_from).max(0);
    if total > 0 {
        let step_per_sec = total as f64 / self.score_anim_duration;
        let inc = (elapsed * step_per_sec).floor() as u32;
        self.display_score = (self.score_anim_from + inc).min(self.score_anim_to);
    }
}

        let offset = self.offset();
        let time = tm.now() as f32;
        let time = match self.state {
            State::Starting => {
                if time >= Self::BEFORE_TIME {
                    self.res.alpha = 1.;
                    self.state = State::BeforeMusic;
                    tm.reset();
                    tm.seek_to(if self.mode == GameMode::Exercise {
                        self.exercise_range.start as f64
                    } else {
                        offset.min(0.) as f64
                    });
                    self.last_update_time = tm.real_time();
                    if self.first_in && self.mode == GameMode::Exercise {
                        tm.pause();
                        self.first_in = false;
                    }
                    tm.now() as f32
                } else {
                    #[cfg(target_os = "windows")]
                    {
                        let emitter_config = self.res.emitter.emitter.config.clone();
                        let emitter_square_config = self.res.emitter.emitter_square.config.clone();
                        self.res.emitter.emitter.config.size = 0.0;
                        self.res.emitter.emitter_square.config.size = 0.0;
                        self.res.emitter.emitter.emit(vec2(0.0, 0.0), 1);
                        self.res.emitter.emitter_square.emit(vec2(0.0, 0.0), 1);
                        self.res.emitter.emitter.config = emitter_config;
                        self.res.emitter.emitter_square.config = emitter_square_config;
                    }
                    self.res.alpha = 1. - (1. - time / Self::BEFORE_TIME).powi(3);
                    if self.mode == GameMode::Exercise {
                        self.exercise_range.start
                    } else {
                        offset
                    }
                }
            }
            State::BeforeMusic => {
                if time >= 0.0 {
                    self.music.seek_to(time)?;
                    if !tm.paused() {
                        self.music.play()?;
                    }
                    self.state = State::Playing;
                }
                time
            }
            State::Playing => {
                if time > self.res.track_length + WAIT_TIME {
                    self.state = State::Ending;
                }
                time
            }
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                if t >= AFTER_TIME + 0.3 {
                    let mut record_data = None;

                    #[cfg(feature = "closed")]
                    if let Some(upload_fn) = &self.upload_fn {
                        if !self.res.config.offline_mode && !self.res.config.autoplay() && self.res.config.speed >= 1.0 - 1e-3 {
                            if let Some(player) = &self.player {
                                if let Some(chart) = &self.res.info.id {
                                    record_data = Some(encode_record(self, player.id, *chart));
                                }
                            }
                        }
                    }
                    let result = self.judge.result();
                    let record = if self.res.config.autoplay() || self.res.config.speed < 1.0 - 1e-3 {
                        None
                    } else {
                        Some(SimpleRecord {
                            score: result.score as _,
                            accuracy: result.accuracy as _,
                            full_combo: result.max_combo == result.num_of_notes,
                        })
                    };
                     self.next_scene = match self.mode {
                        GameMode::Normal | GameMode::NoRetry | GameMode::View => {
                            let historic_best = self.player.as_ref().map_or(0, |it| it.historic_best);
                            if let Some(new_rec) = &record {
                                if let Some(f) = &self.save_fn {
                                    f(new_rec.clone())?;
                                }
                                if let Some(best) = &mut self.best_record {
                                    best.update(new_rec);
                                } else {
                                    self.best_record = record.clone();
                                }
                                if let Some(best) = &self.best_record {
                                    if let Some(player) = &mut self.player {
                                        player.historic_best = player.historic_best.max(best.score as _);
                                    }
                                }
                            }
                            Some(NextScene::Overlay(Box::new(EndingScene::new(
                                self.res.background.clone(),
                                self.res.illustration.clone(),
                                self.res.player.clone(),
                                self.res.icons.clone(),
                                self.res.icon_retry.clone(),
                                self.res.icon_proceed.clone(),
                                self.res.mod_icons.clone(),
                                self.res.info.clone(),
                                self.judge.result(),
                                &self.res.config,
                                self.res.res_pack.ending.clone(),
                                self.upload_fn.as_ref().map(Arc::clone),
                                self.player.as_ref().map(|it| it.rks),
                                historic_best,
                                record_data,
                                self.best_record.clone(),
                                if self.res.config.show_avg_fps { self.get_avg_fps() } else { None },
                            )?)))
                        }
                        GameMode::TweakOffset => Some(NextScene::PopWithResult(Box::new(None::<f32>))),
                        GameMode::Exercise => None,
                    };
                }
                self.res.alpha = 1. - (t / AFTER_TIME).min(1.).powi(2);
                self.res.track_length
            }
        };
        let time = (time - offset).max(0.);
        self.res.time = time;
        if !tm.paused() && self.pause_rewind.is_none() && self.mode != GameMode::View && self.skip_anim_start.is_none() {
            self.gl.quad_gl.viewport(self.res.camera.viewport);
            self.judge.update(&mut self.res, &mut self.chart, &mut self.bad_notes);
            self.gl.quad_gl.viewport(None);
        }

if self.perfect_flash_start.is_none() {
    let counts = self.judge.counts();
    let result = self.judge.result();

    if counts[0] + counts[1] == result.num_of_notes {

        self.perfect_flash_start = Some(tm.now() as f32);
                self.pure_memory_start = Some(tm.now() as f32);

    }
}

        if let Some(update) = &mut self.update_fn {
            update(self.res.time, &mut self.res, &mut self.judge);
        }
        let counts = self.judge.counts();
        self.res.judge_line_color = if counts[3] + counts[4] == 0 {
            Color::from_hex_argb(if counts[2] == 0 {
                self.res.res_pack.info.color_perfect
            } else {
                self.res.res_pack.info.color_good
            })
        } else {
            WHITE
        };
        self.res.judge_line_color.a *= self.res.alpha;
        self.chart.update(&mut self.res);
        let res = &mut self.res;
        if res.config.interactive && is_key_pressed(KeyCode::Space) {
            if tm.paused() {
                if matches!(self.state, State::Playing) {
                    self.music.play()?;
                    tm.resume();
                }
            } else if matches!(self.state, State::Playing | State::BeforeMusic) {
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
            }
        }
        if Self::interactive(res, &self.state) {
            if is_key_pressed(KeyCode::Left) {
                res.time -= 1.;
                let dst = (self.music.position() - 1.).max(0.);
                self.music.seek_to(dst)?;
                tm.seek_to(dst as f64);
            }
            if is_key_pressed(KeyCode::Right) {
                res.time += 5.;
                let dst = (self.music.position() + 5.).min(res.track_length);
                self.music.seek_to(dst)?;
                tm.seek_to(dst as f64);
            }
            if is_key_pressed(KeyCode::Q) {
                self.should_exit = true;
            }
        }
        for e in &mut self.effects {
            e.update(&self.res);
        }
        if let Some((id, text)) = take_input() {
            let offset = self.offset().min(0.);
            match id.as_str() {
                "exercise_start" => {
                    if let Some(t) = parse_time(&text) {
                        if !(offset..self.res.track_length.min(self.exercise_range.end - 3.).max(offset)).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.start = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                "exercise_end" => {
                    if let Some(t) = parse_time(&text) {
                        if !((self.exercise_range.start + 3.).max(offset).min(self.res.track_length)..self.res.track_length).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.end = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                _ => return_input(id, text),
            }
        }

        if self.hp_enabled {
            let now = tm.real_time();
            let dt = (now - self.last_update_time).max(0.) as f32;

            let dt = dt.min(1.0 / 30.0);
            self.update_hp(tm, dt);
        }

        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.mode == GameMode::Exercise && tm.paused() {
            let touch = Touch {
                position: touch.position * self.touch_scale(),
                ..touch.clone()
            };
            if self.exercise_btns.0.touch(&touch) {
                request_input("exercise_start", InputBox::new().default_text(fmt_time(self.exercise_range.start)));
                return Ok(true);
            }
            if self.exercise_btns.1.touch(&touch) {
                request_input("exercise_end", InputBox::new().default_text(fmt_time(self.exercise_range.end)));
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let res = &mut self.res;
        let asp = ui.viewport.2 as f32 / ui.viewport.3 as f32;
        if res.update_size(ui.viewport) || self.mode == GameMode::View {
            set_camera(&res.camera);
        }

        let msaa = res.config.sample_count > 1;

        let chart_onto = res
            .chart_target
            .as_ref()
            .map(|it| if msaa { it.input() } else { it.output() })
            .or(res.camera.render_target);
        push_camera_state();
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            viewport: if res.chart_target.is_some() { None } else { Some(ui.viewport) },
            render_target: chart_onto,
            ..Default::default()
        });
        clear_background(BLACK);
        draw_background(*res.background);
        if let Some(start) = self.perfect_flash_start {
    let elapsed = tm.now() as f32 - start;
    let duration = 1.0;

    if elapsed < duration {

        let p = elapsed / duration;
        let alpha = (1.0 - p).powi(2);

        let h = 1. / res.aspect_ratio;

        draw_rectangle(
            -1.0,
            -h,
            2.0,
            h * 2.0,
            Color::new(
                0.25, 0.55, 1.0,
                alpha * 0.5
            ),
        );
    }
}

        pop_camera_state();

        let chart_target_vp = if res.chart_target.is_some() {
            let vp = res.camera.viewport.unwrap();
            Some((vp.0 - ui.viewport.0, vp.1 - ui.viewport.1, vp.2, vp.3))
        } else {
            res.camera.viewport
        };
        self.gl.quad_gl.render_pass(chart_onto.map(|it| it.render_pass));
        self.gl.quad_gl.viewport(chart_target_vp);

        let h = 1. / res.aspect_ratio;
        draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));

        self.chart.render(ui, res);

        self.gl.quad_gl.render_pass(
            res.chart_target
                .as_ref()
                .map(|it| it.output().render_pass)
                .or_else(|| res.camera.render_pass()),
        );

        self.bad_notes.retain(|dummy| dummy.render(res));
        let t = tm.real_time();
        let dt = (t - std::mem::replace(&mut self.last_update_time, t)) as f32;
        if res.config.particle {
            res.emitter.draw(dt);
        }
        if self.res.config.use_classic_ui {
            self.ui_classic(ui, tm)?;
        } else {
            self.ui(ui, tm)?;
        }
        self.overlay_ui(ui, tm)?;
        if let Some(start) = self.pure_memory_start {
    let t = tm.now() as f32 - start;

    let fade_in = 0.3;
    let hold = 2.0;
    let fade_out = 1.0;
    let total = fade_in + hold + fade_out;

    if t < total {

        let alpha = if t < fade_in {
            (t / fade_in).clamp(0.0, 1.0)
        } else if t < fade_in + hold {
            1.0
        } else {
            let p = (t - fade_in - hold) / fade_out;
            (1.0 - p).clamp(0.0, 1.0)
        };

        let size = if t < fade_in {
            let p = t / fade_in;
            let eased = 1.0 - (1.0 - p).powi(3);
            128.0 - (128.0 - 64.0) * eased
        } else {
            64.0
        };

        ui.text("PURE MEMORY")
            .pos(0.0, 0.0)
            .anchor(0.5, 0.5)
            .size(size / 64.0)
            .color(Color::new(
                0.90, 0.95, 1.00,
                alpha,
            ))
            .draw_using(&PGR_FONT);
    }
}
if let Some(start) = self.perfect_no_miss_start {
    let t = tm.now() as f32 - start;

    let fade_in = 0.3;
    let hold = 2.0;
    let fade_out = 1.0;
    let total = fade_in + hold + fade_out;

    if t < total {
        let alpha = if t < fade_in {
            (t / fade_in).clamp(0.0, 1.0)
        } else if t < fade_in + hold {
            1.0
        } else {
            let p = (t - fade_in - hold) / fade_out;
            (1.0 - p).clamp(0.0, 1.0)
        };

        let size = if t < fade_in {
            let p = t / fade_in;
            let eased = 1.0 - (1.0 - p).powi(3);
            128.0 - (128.0 - 64.0) * eased
        } else {
            64.0
        };

        ui.text("FULL RECALL")
            .pos(0.0, 0.0)
            .anchor(0.5, 0.5)
            .size(size / 64.0)
            .color(Color::new(
                0.75, 0.45, 1.00,
                alpha,
            ))
            .draw_using(&PGR_FONT);
    }
}

        if self.mode == GameMode::TweakOffset {
            push_camera_state();
            self.gl.quad_gl.viewport(None);
            set_camera(&Camera2D {
                zoom: vec2(1., -screen_aspect()),
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            self.tweak_offset(ui, Self::interactive(&self.res, &self.state));
            pop_camera_state();
        }

        if !self.res.no_effect && !self.effects.is_empty() {
            push_camera_state();
            set_camera(&Camera2D {
                zoom: vec2(1., asp),
                ..Default::default()
            });
            for e in &self.effects {
                e.render(&mut self.res);
            }
            pop_camera_state();
        }
        if msaa || !self.res.no_effect {

            if let Some(target) = &self.res.chart_target {
                self.gl.flush();
                push_camera_state();
                self.gl.quad_gl.viewport(None);
                set_camera(&Camera2D {
                    zoom: vec2(1., asp),
                    render_target: self.res.camera.render_target,
                    viewport: Some(ui.viewport),
                    ..Default::default()
                });
                draw_texture_ex(
                    target.output().texture,
                    -1.,
                    -ui.top,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(2., ui.top * 2.)),
                        ..Default::default()
                    },
                );
                pop_camera_state();
            }
        }

        self.render_hp_bar(ui);

        self.render_skip(ui, tm);

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if self.should_exit {
            if tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            match self.mode {
                GameMode::Normal | GameMode::Exercise | GameMode::NoRetry | GameMode::View => NextScene::Pop,
                GameMode::TweakOffset => NextScene::PopWithResult(Box::new(None::<f32>)),
            }
        } else if let Some(next_scene) = self.next_scene.take() {
            tm.speed = 1.0;
            tm.adjust_time = false;
            next_scene
        } else {
            NextScene::None
        }
    }
}
