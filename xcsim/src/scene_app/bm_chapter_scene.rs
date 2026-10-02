use crate::{
    chap_unlock,
    icons::Icons,
    page::{list_chap_chapters, load_chap_charts, ChapInfo, ChartItem, SFader},
    scene::SongScene,
};
use anyhow::Result;
use inputbox::InputBox;
use macroquad::prelude::*;
#[cfg(feature = "video")]
use xcsim_core::{
    core::{demux_audio, Anim, Keyframe, Video},
    ext::{create_audio_manger, ScaleType},
    scene::GameScene,
};
use xcsim_core::{
    config::Mods,
    ext::{
        draw_illustration, draw_parallelogram_ex, draw_text_aligned_opt_width, poll_future, semi_black, semi_white,
        SafeTexture, BLACK_TEXTURE, PARALLELOGRAM_SLOPE,
    },
    scene::{request_input, return_input, show_error, show_message, take_input, BasicPlayer, GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{button_hit, RectButton, Scroll, Ui},
};
#[cfg(feature = "video")]
use sasa::{AudioClip, AudioManager, Music, MusicParams};
use std::sync::Arc;
use tap::Tap;

const UNLOCK_SEQUENCE: &[u8] = &[1, 1, 2, 2, 3, 1, 1, 3];
const UNLOCK_CODE_1: &str = "093";
const UNLOCK_CODE_2: &str = "33550336";
const MAX_CODE_LEN: usize = 16;

const UNLOCK_INPUT_ID: &str = "bm_chap_unlock";

const INPUT_TRIPLE_TAP: u8 = 3;

const IKITE_TEXT: &str = "生きて、生きて、生きて、生きて、生きろ、生きて、生きて、生きて、生きて、生きろ。";

const IKITE_POEM: &str = "巨木何以擢於微種？\n豐碑何以立於餘燼？\n歷經失卻 飲痛瀝血\n天穹尚晦 仍應相搏";
const IKITE_TOTAL: f32 = 5.0;
const IKITE_FADE: f32 = 0.5;

const DIALOG_ENTER: f32 = 0.30;
const LOADING_WHITE_FADE: f32 = 0.5;

const TITLE_BAR_H: f32 = 0.14;
const SIDEBAR_BG: Color = Color::new(0.06, 0.06, 0.06, 0.97);
const DARK_BG: Color = Color::new(0.09, 0.09, 0.09, 0.97);
const ACCENT: Color = crate::theme::FIREFLY_PINK_DEEP;

const LOCKED_NAME: &str = "???";

struct ChapterCard {
    info: ChapInfo,
    btn: RectButton,
}

const RESET_TAPS_REQUIRED: u8 = 6;

struct ResetDialog {
    showing: bool,
    show_t: f32,
    cancel_btn: RectButton,
    reset_btn: RectButton,
}

impl ResetDialog {
    fn new() -> Self {
        Self {
            showing: false,
            show_t: 0.,
            cancel_btn: RectButton::new(),
            reset_btn: RectButton::new(),
        }
    }
    fn open(&mut self, t: f32) { self.showing = true; self.show_t = t; }
    fn close(&mut self) { self.showing = false; }

    fn dialog_rect() -> Rect { Rect::new(-0.50, -0.27, 1.00, 0.54) }
    fn cancel_rect() -> Rect { Rect::new(-0.42, 0.10, 0.36, 0.12) }
    fn reset_rect() -> Rect { Rect::new(0.06, 0.10, 0.36, 0.12) }

    fn draw(&mut self, ui: &mut Ui, t: f32) {

        let elapsed = (t - self.show_t).max(0.);
        let p = (elapsed / DIALOG_ENTER).clamp(0., 1.);
        let p = 1. - (1. - p).powi(3);
        let scale = 0.92 + 0.08 * p;
        let alpha = p;

        ui.fill_rect(ui.screen_rect(), semi_black(0.65 * alpha));

        let dlg = scale_rect(Self::dialog_rect(), scale);
        let cancel = scale_rect(Self::cancel_rect(), scale);
        let reset = scale_rect(Self::reset_rect(), scale);

        draw_parallelogram_ex(
            Rect::new(dlg.x - 0.008, dlg.y - 0.008, dlg.w + 0.016, dlg.h + 0.016),
            None,
            with_alpha(semi_black(0.95), alpha),
            with_alpha(semi_black(0.95), alpha),
            true,
        );
        draw_parallelogram_ex(
            dlg,
            None,
            with_alpha(Color::new(0.165, 0.110, 0.180, 1.0), alpha),
            with_alpha(Color::new(0.06, 0.06, 0.06, 1.0), alpha),
            false,
        );
        let accent = Color::new(0.85, 0.20, 0.20, 1.0);
        let accent_strip = Rect::new(dlg.x, dlg.y, dlg.w - dlg.h * PARALLELOGRAM_SLOPE, 0.012);
        draw_parallelogram_ex(accent_strip, None, with_alpha(accent, alpha), with_alpha(accent, alpha), false);

        ui.text("DEBUG · Reset Chapter Progress")
            .pos(0., dlg.y + 0.06)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.70)
            .color(with_alpha(WHITE, alpha))
            .draw();
        ui.text("This will re-lock every chapter chart.")
            .pos(0., dlg.y + 0.16)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.46)
            .color(with_alpha(semi_white(0.65), alpha))
            .max_width(dlg.w - 0.10)
            .draw();

        draw_parallelogram_ex(
            cancel,
            None,
            with_alpha(Color::new(0.20, 0.20, 0.20, 1.0), alpha),
            with_alpha(Color::new(0.14, 0.14, 0.14, 1.0), alpha),
            true,
        );
        ui.text("Cancel")
            .pos(cancel.center().x, cancel.center().y)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.55)
            .color(with_alpha(semi_white(0.85), alpha))
            .draw();
        self.cancel_btn.set(ui, cancel);

        draw_parallelogram_ex(
            reset,
            None,
            with_alpha(accent, alpha),
            with_alpha(Color::new(accent.r * 0.6, accent.g * 0.6, accent.b * 0.6, 1.0), alpha),
            true,
        );
        ui.text("Reset")
            .pos(reset.center().x, reset.center().y)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.62)
            .color(with_alpha(WHITE, alpha))
            .draw();
        self.reset_btn.set(ui, reset);
    }
}

pub struct ChaptersScene {
    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    btn_back: RectButton,
    chapters: Vec<ChapterCard>,

    scroll: Scroll,
    sf: SFader,

    next_scene: Option<NextScene>,
    first_in: bool,

    title_btn: RectButton,
    title_taps: u8,
    reset_dialog: ResetDialog,
}

impl ChaptersScene {
    const W: f32 = 0.5;
    const H: f32 = 0.55;
    const PAD: f32 = 0.06;

    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let chapters = list_chap_chapters()
            .into_iter()
            .map(|info| ChapterCard { info, btn: RectButton::new() })
            .collect();
        Self {
            icons,
            rank_icons,
            btn_back: RectButton::new(),
            chapters,
            scroll: Scroll::new().horizontal().tap_mut(|it| it.x_scroller.step = Self::W + Self::PAD),
            sf: SFader::new(),
            next_scene: None,
            first_in: true,
            title_btn: RectButton::new(),
            title_taps: 0,
            reset_dialog: ResetDialog::new(),
        }
    }
}

impl Scene for ChaptersScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.first_in {
            self.first_in = false;
            tm.reset();
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;

        if self.reset_dialog.showing {
            if self.reset_dialog.reset_btn.touch(touch) {
                button_hit();
                chap_unlock::reset_all();
                self.reset_dialog.close();
                self.title_taps = 0;
                show_message("All chapter progress reset.").ok();
                return Ok(true);
            }
            if self.reset_dialog.cancel_btn.touch(touch) {
                button_hit();
                self.reset_dialog.close();
                self.title_taps = 0;
                return Ok(true);
            }
            return Ok(true);
        }

        if self.title_btn.touch(touch) {
            button_hit();
            self.title_taps = self.title_taps.saturating_add(1);
            if self.title_taps >= RESET_TAPS_REQUIRED {
                self.title_taps = 0;
                self.reset_dialog.open(t);
            }
            return Ok(true);
        }

        if self.btn_back.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::Pop);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        for chap in &mut self.chapters {
            if chap.btn.touch(touch) {
                button_hit();
                let scene = ChapterChartsScene::new(
                    chap.info.id.clone(),
                    chap.info.name.clone(),
                    Arc::clone(&self.icons),
                    self.rank_icons.clone(),
                );
                self.sf.goto(t, scene);
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.scroll.update(t);
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;

        let r = ui.screen_rect();
        ui.fill_rect(r, BLACK);

        let bar_y = -ui.top;
        ui.fill_rect(Rect::new(-1., bar_y, 2., TITLE_BAR_H), SIDEBAR_BG);

        let r_back = ui.back_rect();
        ui.fill_rect(r_back, (*self.icons.back, r_back));
        self.btn_back.set(ui, r_back);

        ui.text("BM Chapter")
            .pos(0., bar_y + TITLE_BAR_H * 0.5)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.7)
            .draw();

        self.title_btn
            .set(ui, Rect::new(-0.30, bar_y, 0.60, TITLE_BAR_H));

        if self.chapters.is_empty() {
            ui.text("No chapters in assets/charts/chap")
                .pos(0., 0.)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.5)
                .color(semi_white(0.6))
                .draw();
            self.sf.render(ui, t);
            return Ok(());
        }

        let body_r = Rect::new(-1., bar_y + TITLE_BAR_H, 2., ui.top * 2. - TITLE_BAR_H);
        self.scroll.size((body_r.w, body_r.h));
        ui.scope(|ui| {
            ui.dx(body_r.x);
            ui.dy(body_r.y);
            self.scroll.render(ui, |ui| {
                ui.dx(1.);
                ui.dy(body_r.h * 0.5);
                let mut x = 0.;
                let step = Self::W + Self::PAD;
                for chap in &mut self.chapters {
                    let cr = Rect::new(x - Self::W / 2., -Self::H / 2., Self::W, Self::H);
                    chap.btn.set(ui, cr);
                    ui.fill_rect(cr, DARK_BG);
                    ui.fill_rect(Rect::new(cr.x, cr.bottom() - 0.004, cr.w, 0.004), ACCENT);
                    ui.text(&chap.info.name)
                        .pos(cr.x + 0.03, cr.y + 0.03)
                        .max_width(cr.w - 0.06)
                        .size(0.7)
                        .color(WHITE)
                        .draw();
                    if chap.info.name != chap.info.id {
                        ui.text(&chap.info.id)
                            .pos(cr.x + 0.03, cr.y + 0.13)
                            .max_width(cr.w - 0.06)
                            .size(0.34)
                            .color(semi_white(0.45))
                            .draw();
                    }
                    ui.text("Tap to open")
                        .pos(cr.x + 0.03, cr.bottom() - 0.04)
                        .size(0.4)
                        .color(semi_white(0.55))
                        .draw();
                    x += step;
                }
                (step * self.chapters.len() as f32, body_r.h)
            });
        });

        self.sf.render(ui, t);

        if self.reset_dialog.showing {
            self.reset_dialog.draw(ui, t);
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        self.next_scene
            .take()
            .or_else(|| self.sf.next_scene(tm.now() as f32))
            .unwrap_or_default()
    }
}

struct ChartRow {

    item: Option<ChartItem>,
    btn: RectButton,
    locked: bool,
}

const TARGET_ROW_COUNT: usize = 6;

struct UnlockDialog {
    showing: bool,
    text: String,
    confirm_btn: RectButton,
    cancel_btn: RectButton,

    input_btn: RectButton,

    input_taps: u8,
    show_t: f32,
}

impl UnlockDialog {
    fn new() -> Self {
        Self {
            showing: false,
            text: String::new(),
            confirm_btn: RectButton::new(),
            cancel_btn: RectButton::new(),
            input_btn: RectButton::new(),
            input_taps: 0,
            show_t: 0.,
        }
    }

    fn open(&mut self, t: f32) {
        self.showing = true;
        self.text.clear();
        self.input_taps = 0;
        self.show_t = t;
    }

    fn close(&mut self) {
        self.showing = false;
        self.text.clear();
        self.input_taps = 0;
    }

    fn dialog_rect() -> Rect { Rect::new(-0.45, -0.27, 0.90, 0.54) }
    fn input_rect() -> Rect { Rect::new(-0.36, -0.07, 0.72, 0.14) }
    fn cancel_rect() -> Rect { Rect::new(-0.36, 0.12, 0.34, 0.10) }
    fn confirm_rect() -> Rect { Rect::new(0.02, 0.12, 0.34, 0.10) }
}

pub struct ChapterChartsScene {
    chapter_name: String,
    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    btn_back: RectButton,
    rows: Vec<ChartRow>,
    scroll: Scroll,
    sf: SFader,

    next_scene: Option<NextScene>,
    first_in: bool,

    tap_seq: Vec<u8>,
    dialog: UnlockDialog,
    ikite_start: Option<f32>,
    pending_local_path: Option<String>,

    pending_direct_game: bool,
    pending_unlock: Option<String>,
    scene_task: xcsim_core::scene::LocalSceneTask,

    row06_taps: u8,
    reset_dialog: ResetDialog,
}

impl ChapterChartsScene {
    const ILLU_W_MULT: f32 = 0.5;
    const ILLU_H_MULT: f32 = 0.5;
    const ROW_PAD: f32 = 0.04;

    pub fn new(chapter_id: String, chapter_name: String, icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let mut rows: Vec<ChartRow> = load_chap_charts(&chapter_id)
            .into_iter()
            .take(TARGET_ROW_COUNT)
            .map(|item| {
                let locked = item
                    .local_path
                    .as_deref()
                    .map(|p| !chap_unlock::is_unlocked(p))
                    .unwrap_or(true);
                ChartRow {
                    item: Some(item),
                    btn: RectButton::new(),
                    locked,
                }
            })
            .collect();

        while rows.len() < TARGET_ROW_COUNT {
            rows.push(ChartRow {
                item: None,
                btn: RectButton::new(),
                locked: true,
            });
        }
        Self {
            chapter_name,
            icons,
            rank_icons,
            btn_back: RectButton::new(),
            rows,
            scroll: Scroll::new(),
            sf: SFader::new(),
            next_scene: None,
            first_in: true,
            tap_seq: Vec::with_capacity(UNLOCK_SEQUENCE.len()),
            dialog: UnlockDialog::new(),
            ikite_start: None,
            pending_local_path: None,
            pending_direct_game: false,
            pending_unlock: None,
            scene_task: None,
            row06_taps: 0,
            reset_dialog: ResetDialog::new(),
        }
    }

    fn row_height() -> f32 {
        0.076 * 7. * Self::ILLU_H_MULT + Self::ROW_PAD
    }

    fn try_confirm_code(&mut self, t: f32) {
        let entered = self.dialog.text.clone();
        let (chart_idx, direct_game) = if entered == UNLOCK_CODE_1 {
            (0usize, false)
        } else if entered == UNLOCK_CODE_2 {
            (1usize, true)
        } else {
            self.dialog.text.clear();
            show_message("Incorrect code.").error();
            return;
        };
        let Some(item) = self.rows.get(chart_idx).and_then(|r| r.item.as_ref()) else {
            self.dialog.close();
            show_message("That chart slot is empty.").error();
            return;
        };
        let Some(local_path) = item.local_path.clone() else {
            self.dialog.close();
            show_message("Missing chart path.").error();
            return;
        };
        self.dialog.close();
        self.pending_local_path = Some(local_path);
        self.pending_direct_game = direct_game;
        self.ikite_start = Some(t);
    }

    fn refresh_locks(&mut self) {
        for row in &mut self.rows {
            if let Some(item) = &row.item {
                if let Some(path) = item.local_path.as_deref() {
                    row.locked = !chap_unlock::is_unlocked(path);
                }
            } else {
                row.locked = true;
            }
        }
    }
}

impl Scene for ChapterChartsScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        let was_first = self.first_in;
        if self.first_in {
            self.first_in = false;
            tm.reset();
        }
        if !was_first {

            if let Some(path) = self.pending_unlock.take() {
                chap_unlock::unlock(&path);
                self.refresh_locks();
            }
        }
        Ok(())
    }

    fn on_result(&mut self, _tm: &mut TimeManager, _result: Box<dyn std::any::Any>) -> Result<()> {

        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;

        if self.ikite_start.is_some() {
            return Ok(true);
        }

        if self.reset_dialog.showing {
            if self.reset_dialog.reset_btn.touch(touch) {
                button_hit();
                chap_unlock::reset_all();
                self.refresh_locks();
                self.reset_dialog.close();
                self.row06_taps = 0;
                return Ok(true);
            }
            if self.reset_dialog.cancel_btn.touch(touch) {
                button_hit();
                self.reset_dialog.close();
                self.row06_taps = 0;
                return Ok(true);
            }
            return Ok(true);
        }

        if self.dialog.showing {
            if self.dialog.confirm_btn.touch(touch) {
                button_hit();
                self.try_confirm_code(t);
                return Ok(true);
            }
            if self.dialog.cancel_btn.touch(touch) {
                button_hit();
                self.dialog.close();
                return Ok(true);
            }
            if self.dialog.input_btn.touch(touch) {
                button_hit();

                let mobile = cfg!(any(target_os = "android", target_os = "ios"));
                self.dialog.input_taps = self.dialog.input_taps.saturating_add(1);
                if mobile || self.dialog.input_taps >= INPUT_TRIPLE_TAP {
                    self.dialog.input_taps = 0;
                    request_input(UNLOCK_INPUT_ID, InputBox::new().default_text(&self.dialog.text));
                }
                return Ok(true);
            }
            return Ok(true);
        }

        if self.btn_back.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::Pop);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        for (idx, row) in self.rows.iter_mut().enumerate() {
            if row.btn.touch(touch) {
                button_hit();
                let position = (idx + 1) as u8;
                self.tap_seq.push(position);
                if self.tap_seq.len() > UNLOCK_SEQUENCE.len() {
                    self.tap_seq.remove(0);
                }
                if self.tap_seq.as_slice() == UNLOCK_SEQUENCE {
                    self.tap_seq.clear();
                    self.dialog.open(t);
                    return Ok(true);
                }

                if idx == 2 {
                    self.row06_taps = self.row06_taps.saturating_add(1);
                    if self.row06_taps >= 6 {
                        self.row06_taps = 0;
                        self.reset_dialog.open(t);
                        return Ok(true);
                    }
                }

                if row.locked {

                    return Ok(true);
                }
                let Some(item) = row.item.as_ref() else {

                    return Ok(true);
                };
                let scene = SongScene::new(
                    item.clone(),
                    item.local_path.clone(),
                    Arc::clone(&self.icons),
                    self.rank_icons.clone(),
                    Mods::empty(),
                );
                self.sf.goto(t, scene);
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.scroll.update(t);
        for row in &mut self.rows {
            if !row.locked {
                if let Some(item) = row.item.as_mut() {
                    item.illu.settle(t);
                }
            }
        }

        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.scene_task = None;
                match res {
                    Err(err) => {

                        self.pending_unlock = None;
                        show_error(err);
                    }
                    Ok(scene) => {
                        self.next_scene = Some(scene);
                    }
                }
            }
        }

        if let Some(start) = self.ikite_start {
            if t - start >= IKITE_TOTAL {
                self.ikite_start = None;
                if let Some(path) = self.pending_local_path.take() {
                    let direct_game = self.pending_direct_game;
                    self.pending_unlock = Some(path.clone());
                    self.scene_task = Some(Box::pin(launch_chapter_chart_with_video(path, direct_game)));
                }
            }
        }

        if self.dialog.showing {
            while let Some(c) = get_char_pressed() {
                if c.is_ascii_alphanumeric() && self.dialog.text.len() < MAX_CODE_LEN {
                    self.dialog.text.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.dialog.text.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                self.try_confirm_code(t);
            } else if is_key_pressed(KeyCode::Escape) {
                self.dialog.close();
            }
        } else {
            while get_char_pressed().is_some() {}
        }

        if let Some((id, text)) = take_input() {
            if id == UNLOCK_INPUT_ID {
                if self.dialog.showing {
                    self.dialog.text = text.chars().take(MAX_CODE_LEN).collect();
                    self.try_confirm_code(t);
                }

            } else {
                return_input(id, text);
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;

        let r = ui.screen_rect();
        ui.fill_rect(r, BLACK);

        let bar_y = -ui.top;
        ui.fill_rect(Rect::new(-1., bar_y, 2., TITLE_BAR_H), SIDEBAR_BG);
        ui.fill_rect(Rect::new(-1., bar_y + TITLE_BAR_H - 0.0015, 2., 0.0015), ACCENT);

        let r_back = ui.back_rect();
        ui.fill_rect(r_back, (*self.icons.back, r_back));
        self.btn_back.set(ui, r_back);

        ui.text(&self.chapter_name)
            .pos(0., bar_y + TITLE_BAR_H * 0.5)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.7)
            .draw();

        let body_r = Rect::new(-0.97, bar_y + TITLE_BAR_H + 0.01, 1.94, ui.top * 2. - TITLE_BAR_H - 0.02);
        ui.fill_rect(body_r, DARK_BG);

        if self.rows.is_empty() {
            ui.text("No charts in this chapter")
                .pos(body_r.center().x, body_r.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.5)
                .color(semi_white(0.6))
                .draw();
            self.sf.render(ui, t);
            return Ok(());
        }

        let row_h = Self::row_height();
        let total_h = row_h * self.rows.len() as f32 + Self::ROW_PAD;

        let illu_w_world = 0.076 * 13. * Self::ILLU_W_MULT;
        let illu_h_world = 0.076 * 7. * Self::ILLU_H_MULT;

        ui.scissor(body_r, |ui| {
            self.scroll.size((body_r.w, body_r.h));
            ui.scope(|ui| {
                ui.dx(body_r.x);
                ui.dy(body_r.y);
                self.scroll.render(ui, |ui| {
                    let mut y = Self::ROW_PAD;
                    for (idx, row) in self.rows.iter_mut().enumerate() {

                        let local_cx = body_r.w * 0.5;
                        let local_cy = y + row_h * 0.5;
                        let local_illu_r = Rect::new(
                            local_cx - illu_w_world * 0.5,
                            local_cy - illu_h_world * 0.5,
                            illu_w_world,
                            illu_h_world,
                        );

                        let world_illu_r = ui.rect_to_global(local_illu_r);
                        let world_cx = world_illu_r.center().x;
                        let world_cy = world_illu_r.center().y;

                        let illu_tex: Texture2D = if row.locked {
                            **BLACK_TEXTURE
                        } else if let Some(item) = row.item.as_mut() {
                            item.illu.notify();
                            *item.illu.texture.0
                        } else {
                            **BLACK_TEXTURE
                        };

                        let illu_r = draw_illustration(
                            illu_tex,
                            world_cx,
                            world_cy,
                            Self::ILLU_W_MULT,
                            Self::ILLU_H_MULT,
                            WHITE,
                            true,
                        );

                        let ratio = 0.22;
                        let strip = Rect::new(
                            illu_r.x,
                            illu_r.y + illu_r.h * (1. - ratio),
                            illu_r.w - illu_r.h * (1. - ratio) * PARALLELOGRAM_SLOPE,
                            illu_r.h * ratio,
                        );
                        draw_parallelogram_ex(
                            strip,
                            None,
                            Color::default(),
                            Color::new(0., 0., 0., 0.78),
                            true,
                        );

                        let prefix = match idx {
                            0 => "3",
                            1 => "5",
                            2 => "06",
                            _ => "",
                        };
                        let title: String = if row.locked || row.item.is_none() {
                            if prefix.is_empty() {
                                LOCKED_NAME.to_string()
                            } else {
                                format!("{prefix}   {LOCKED_NAME}")
                            }
                        } else {

                            row.item
                                .as_ref()
                                .map(|it| it.info.name.clone())
                                .unwrap_or_else(|| LOCKED_NAME.to_string())
                        };

                        let text_x = local_illu_r.x + local_illu_r.w * 0.06;
                        let text_y = local_illu_r.bottom() - local_illu_r.h * 0.045;
                        let max_w = local_illu_r.w * 0.78;
                        draw_text_aligned_opt_width(ui, &title, text_x, text_y, (0., 1.), 0.92, WHITE, max_w);

                        row.btn.set(ui, local_illu_r);

                        y += row_h;
                    }
                    (body_r.w, total_h)
                });
            });
        });

        self.sf.render(ui, t);

        if self.dialog.showing {
            self.draw_dialog(ui, t);
        }
        if self.reset_dialog.showing {
            self.reset_dialog.draw(ui, t);
        }
        if let Some(start) = self.ikite_start {
            self.draw_ikite(ui, t - start);
        }

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        self.next_scene
            .take()
            .or_else(|| self.sf.next_scene(tm.now() as f32))
            .unwrap_or_default()
    }
}

impl ChapterChartsScene {
    fn draw_dialog(&mut self, ui: &mut Ui, t: f32) {

        let elapsed = (t - self.dialog.show_t).max(0.);
        let p = (elapsed / DIALOG_ENTER).clamp(0., 1.);
        let p = 1. - (1. - p).powi(3);
        let scale = 0.92 + 0.08 * p;
        let alpha = p;

        ui.fill_rect(ui.screen_rect(), semi_black(0.65 * alpha));

        let dlg = UnlockDialog::dialog_rect();

        let dlg = scale_rect(dlg, scale);
        let inp = scale_rect(UnlockDialog::input_rect(), scale);
        let cancel = scale_rect(UnlockDialog::cancel_rect(), scale);
        let confirm = scale_rect(UnlockDialog::confirm_rect(), scale);

        draw_parallelogram_ex(
            Rect::new(dlg.x - 0.008, dlg.y - 0.008, dlg.w + 0.016, dlg.h + 0.016),
            None,
            with_alpha(semi_black(0.95), alpha),
            with_alpha(semi_black(0.95), alpha),
            true,
        );
        draw_parallelogram_ex(
            dlg,
            None,
            with_alpha(Color::new(0.165, 0.110, 0.180, 1.0), alpha),
            with_alpha(Color::new(0.06, 0.06, 0.06, 1.0), alpha),
            false,
        );

        let accent_strip = Rect::new(dlg.x, dlg.y, dlg.w - dlg.h * PARALLELOGRAM_SLOPE, 0.012);
        draw_parallelogram_ex(accent_strip, None, with_alpha(ACCENT, alpha), with_alpha(ACCENT, alpha), false);

        ui.text("Enter Code")
            .pos(0., dlg.y + 0.06)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.85)
            .color(with_alpha(WHITE, alpha))
            .draw();

        draw_parallelogram_ex(
            inp,
            None,
            with_alpha(Color::new(0.18, 0.18, 0.18, 1.0), alpha),
            with_alpha(Color::new(0.12, 0.12, 0.12, 1.0), alpha),
            true,
        );
        let cursor_visible = ((t - self.dialog.show_t) % 1.0) < 0.5;
        let mut shown = self.dialog.text.clone();
        shown.push(if cursor_visible { '_' } else { ' ' });
        draw_text_aligned_opt_width(
            ui,
            &shown,
            inp.x + inp.w * 0.08,
            inp.center().y,
            (0., 0.5),
            0.95,
            with_alpha(WHITE, alpha),
            inp.w * 0.84,
        );

        self.dialog.input_btn.set(ui, inp);

        draw_parallelogram_ex(
            cancel,
            None,
            with_alpha(Color::new(0.20, 0.20, 0.20, 1.0), alpha),
            with_alpha(Color::new(0.14, 0.14, 0.14, 1.0), alpha),
            true,
        );
        ui.text("Cancel")
            .pos(cancel.center().x, cancel.center().y)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.55)
            .color(with_alpha(semi_white(0.85), alpha))
            .draw();
        self.dialog.cancel_btn.set(ui, cancel);

        draw_parallelogram_ex(
            confirm,
            None,
            with_alpha(Color::new(ACCENT.r, ACCENT.g, ACCENT.b, 1.0), alpha),
            with_alpha(Color::new(ACCENT.r * 0.7, ACCENT.g * 0.7, ACCENT.b * 0.7, 1.0), alpha),
            true,
        );
        ui.text("Confirm")
            .pos(confirm.center().x, confirm.center().y)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.6)
            .color(with_alpha(WHITE, alpha))
            .draw();
        self.dialog.confirm_btn.set(ui, confirm);
    }

    fn draw_ikite(&mut self, ui: &mut Ui, elapsed: f32) {

        let alpha = if elapsed < IKITE_FADE {
            (elapsed / IKITE_FADE).clamp(0., 1.)
        } else if elapsed > IKITE_TOTAL - IKITE_FADE {
            ((IKITE_TOTAL - elapsed) / IKITE_FADE).clamp(0., 1.)
        } else {
            1.
        };

        ui.fill_rect(ui.screen_rect(), Color::new(0., 0., 0., alpha));

        let p_in = (elapsed / IKITE_FADE).clamp(0., 1.);
        let p_in = 1. - (1. - p_in).powi(3);
        let _scale = 0.96 + 0.04 * p_in;

        let text = if self.pending_direct_game { IKITE_POEM } else { IKITE_TEXT };
        ui.text(text)
            .pos(0., 0.)
            .anchor(0.5, 0.5)
            .no_baseline()
            .max_width(1.6)
            .multiline()
            .size(0.68)
            .color(with_alpha(WHITE, alpha))
            .draw();
    }
}

fn scale_rect(r: Rect, s: f32) -> Rect {
    let cx = r.x + r.w * 0.5;
    let cy = r.y + r.h * 0.5;
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

async fn launch_chapter_chart_with_video(local_path: String, direct_game: bool) -> Result<NextScene> {
    #[cfg(not(feature = "video"))]
    {
        let _ = direct_game;
        return launch_chapter_chart_masked(local_path).await;
    }
    #[cfg(feature = "video")]
    {
        use std::pin::Pin;
        use std::future::Future;

        let mut fs = super::fs_from_path(&local_path)?;
        let mut info = xcsim_core::fs::load_info(fs.as_mut()).await?;
        info.name = "???".into();
        info.composer = "???".into();
        info.charter = "???".into();
        info.illustrator = "???".into();
        info.level = "???".into();
        info.tip = Some(String::new());

        let mut config = crate::get_data().config.clone();
        config.player_name = crate::get_data()
            .me
            .as_ref()
            .map(|it| it.name.clone())
            .unwrap_or_else(|| "Guest".to_string());
        config.mods = Mods::empty();

        let preload = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
        let player = crate::get_data().me.as_ref().map(|it| BasicPlayer {
            avatar: None,
            id: it.id,
            rks: it.rks,
            historic_best: 0,
        });

        let scene_future: Pin<Box<dyn Future<Output = Result<Box<dyn Scene>>>>> = if direct_game {

            let illu = preload.0.clone();
            let bg = preload.1.clone();
            Box::pin(async move {
                let mut game = GameScene::new(
                    GameMode::Normal,
                    info,
                    config,
                    fs,
                    player,
                    bg,
                    illu,
                    None,
                    None,
                    None,
                )
                .await?;
                game.set_pause_disabled(true);
                Ok(Box::new(game) as Box<dyn Scene>)
            })
        } else {

            Box::pin(async move {
                let mut loading = LoadingScene::new(
                    GameMode::Normal,
                    info,
                    config,
                    fs,
                    player,
                    None,
                    None,
                    None,
                    Some(preload),
                )
                .await?;
                loading.set_white_fade_in(LOADING_WHITE_FADE);
                loading.set_pause_disabled(true);
                Ok(Box::new(loading) as Box<dyn Scene>)
            })
        };

        match try_load_intro_media().await {
            Some(media) => {
                let intro = UnlockIntroScene::new(media, scene_future);
                Ok(NextScene::Overlay(Box::new(intro)))
            }
            None => {
                let scene = scene_future.await?;
                Ok(NextScene::Overlay(scene))
            }
        }
    }
}

#[cfg(not(feature = "video"))]
async fn launch_chapter_chart_masked(local_path: String) -> Result<NextScene> {
    let mut fs = super::fs_from_path(&local_path)?;
    let mut info = xcsim_core::fs::load_info(fs.as_mut()).await?;
    info.name = "???".into();
    info.composer = "???".into();
    info.charter = "???".into();
    info.illustrator = "???".into();
    info.level = "???".into();
    info.tip = Some(String::new());

    let mut config = crate::get_data().config.clone();
    config.player_name = crate::get_data()
        .me
        .as_ref()
        .map(|it| it.name.clone())
        .unwrap_or_else(|| "Guest".to_string());

    let preload = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
    let player = crate::get_data().me.as_ref().map(|it| BasicPlayer {
        avatar: None,
        id: it.id,
        rks: it.rks,
        historic_best: 0,
    });

    let mut loading = LoadingScene::new(GameMode::Normal, info, config, fs, player, None, None, None, Some(preload)).await?;
    loading.set_white_fade_in(LOADING_WHITE_FADE);
    Ok(NextScene::Overlay(Box::new(loading)))
}

#[cfg(feature = "video")]
struct IntroBgm {
    audio_manager: AudioManager,
    music: Music,
}

#[cfg(feature = "video")]
enum IntroState {
    Before,
    Playing,
    Done,
}

#[cfg(feature = "video")]
pub struct UnlockIntroScene {
    video: Video,
    bgm: Option<IntroBgm>,
    music_length: f32,

    scene_task: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn Scene>>>>>>,
    pending_scene: Option<Box<dyn Scene>>,
    next_scene: Option<NextScene>,

    state: IntroState,
    render_target: Option<RenderTarget>,
}

#[cfg(feature = "video")]
struct IntroMedia {
    video: Video,
    bgm: Option<IntroBgm>,
    music_length: f32,
}

#[cfg(feature = "video")]
async fn try_load_intro_media() -> Option<IntroMedia> {
    let bytes = match load_file("unlock.mp4").await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("unlock.mp4 not found, skipping intro: {e}");
            return None;
        }
    };
    let video = match Video::new(
        bytes,
        0.,
        ScaleType::Inside,
        Anim::new(vec![Keyframe::new(0., 1., 0)]),
        Anim::default(),
    ) {
        Ok(v) => v,
        Err(e) => {

            tracing::warn!("unlock.mp4 failed to decode, skipping intro: {e:#}");
            return None;
        }
    };

    let clip = demux_audio(video.video_file().path().to_str().unwrap()).ok().flatten();
    let music_length = clip.as_ref().map_or(0., AudioClip::length);
    let config = crate::get_data().config.clone();
    let bgm = match clip {
        Some(clip) => match create_audio_manger(&config) {
            Ok(mut audio_manager) => match audio_manager.create_music(
                clip,
                MusicParams {
                    amplifier: config.volume_music,
                    ..Default::default()
                },
            ) {
                Ok(music) => Some(IntroBgm { audio_manager, music }),
                Err(e) => {
                    tracing::warn!("unlock.mp4 audio init failed: {e:#}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("unlock.mp4 audio manager failed: {e:#}");
                None
            }
        },
        None => None,
    };

    Some(IntroMedia { video, bgm, music_length })
}

#[cfg(feature = "video")]
impl UnlockIntroScene {
    pub fn new(
        media: IntroMedia,
        scene_future: std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn Scene>>>>>,
    ) -> Self {
        Self {
            video: media.video,
            bgm: media.bgm,
            music_length: media.music_length,
            scene_task: Some(scene_future),
            pending_scene: None,
            next_scene: None,
            state: IntroState::Before,
            render_target: None,
        }
    }
}

#[cfg(feature = "video")]
impl Scene for UnlockIntroScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.render_target = target;
        tm.reset();
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.pause();
        if let Some(bgm) = &mut self.bgm {
            bgm.music.pause()?;
        }
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        if let Some(bgm) = &mut self.bgm {
            bgm.music.play()?;
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            bgm.audio_manager.recover_if_needed()?;
        }

        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.scene_task = None;
                match res {
                    Err(err) => {

                        show_error(err);
                        self.next_scene = Some(NextScene::Pop);
                        return Ok(());
                    }
                    Ok(scene) => {
                        self.pending_scene = Some(scene);
                    }
                }
            }
        }

        let t = tm.now() as f32;
        match self.state {
            IntroState::Before => {
                if t > 0.3 {
                    self.state = IntroState::Playing;
                    tm.reset();
                    if let Some(bgm) = &mut self.bgm {
                        bgm.music.seek_to(0.)?;
                        bgm.music.play()?;
                    }
                }
            }
            IntroState::Playing => {
                if self.video.ended && t > self.music_length {
                    self.state = IntroState::Done;
                } else {
                    if let Some(bgm) = &mut self.bgm {
                        tm.update(bgm.music.position() as _);
                    }
                    self.video.update(t)?;
                }
            }
            IntroState::Done => {
                if let Some(scene) = self.pending_scene.take() {
                    self.next_scene = Some(NextScene::Replace(scene));
                }

            }
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let t = tm.now() as f32;
        cam.render_target = self.render_target;
        set_camera(&cam);
        clear_background(BLACK);

        if matches!(self.state, IntroState::Playing) && t > 0.05 {
            self.video.render(t, asp, WHITE);
        }
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().unwrap_or(NextScene::None)
    }
}
