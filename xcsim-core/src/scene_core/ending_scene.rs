xcsim_core_l10n::tl_file!("ending");

use super::{draw_background, game::SimpleRecord, loading::UploadFn, NextScene, Scene};
use crate::{
    config::{Config, Mods},
    ext::{create_audio_manger, draw_parallelogram, draw_illustration, draw_text_aligned,draw_parallelogram_ex, draw_text_aligned_opt_width, SafeTexture,  PARALLELOGRAM_SLOPE},
    info::ChartInfo,
    judge::{icon_index, PlayResult},
    scene::show_message,
    task::Task,
    time::TimeManager,
    ui::{RectButton, Dialog, MessageHandle, Ui},
};
use anyhow::Result;
use macroquad::prelude::*;
use sasa::{AudioClip, AudioManager, Music, MusicParams};
use serde::Deserialize;
use std::{cell::RefCell, ops::DerefMut};

#[derive(Deserialize)]
pub struct RecordUpdateState {
    pub best: bool,
    pub improvement: u32,
    pub gain_exp: f32,
    pub new_rks: Option<f32>,
}

pub struct EndingScene {
    background: SafeTexture,
    illustration: SafeTexture,
    player: SafeTexture,
    icons: [SafeTexture; 8],
    icon_retry: SafeTexture,
    icon_proceed: SafeTexture,
    mod_icons: [SafeTexture; 6],
    target: Option<RenderTarget>,
    audio: AudioManager,
    bgm: Music,

    info: ChartInfo,
    result: PlayResult,
    player_name: String,
    player_rks: Option<f32>,
    autoplay: bool,
    use_keyboard: bool,
    speed: f32,
    mods: Mods,
    next: u8,
    update_state: Option<RecordUpdateState>,
    rated: bool,

    upload_fn: Option<UploadFn>,
    upload_task: Option<(Task<Result<RecordUpdateState>>, MessageHandle)>,
    record_data: Option<Vec<u8>>,
    best_record: Option<SimpleRecord>,

    btn_retry: RectButton,
    btn_proceed: RectButton,
    btn_detail: RectButton,
    detail_mode: bool,

    tr_start: f32,

    avg_fps: Option<f32>,
}
impl EndingScene {
    pub fn new(
        background: SafeTexture,
        illustration: SafeTexture,
        player: SafeTexture,
        icons: [SafeTexture; 8],
        icon_retry: SafeTexture,
        icon_proceed: SafeTexture,
        mod_icons: [SafeTexture; 6],
        info: ChartInfo,
        result: PlayResult,
        config: &Config,
        bgm: AudioClip,
        upload_fn: Option<UploadFn>,
        player_rks: Option<f32>,
        historic_best: u32,
        record_data: Option<Vec<u8>>,
        best_record: Option<SimpleRecord>,
        avg_fps: Option<f32>,
    ) -> Result<Self> {
        let mut audio = create_audio_manger(config)?;
        let bgm = audio.create_music(
            bgm,
            MusicParams {
                amplifier: config.volume_music,
                loop_mix_time: 0.,
                ..Default::default()
            },
        )?;
        let upload_task = upload_fn
            .as_ref()
            .and_then(|f| record_data.clone().map(|data| (f(data), show_message(tl!("uploading")).handle())));
        Ok(Self {
            background,
            illustration,
            player,
            icons,
            icon_retry,
            icon_proceed,
            mod_icons,
            target: None,
            audio,
            bgm,
            update_state: if upload_task.is_some() {
                None
            } else {
                let (best, improvement) = if result.score > historic_best {
                    (true, result.score - historic_best)
                } else {
                    (false, 0)
                };
                Some(RecordUpdateState {
                    best,
                    improvement,
                    gain_exp: 0.,
                    new_rks: None,
                })
            },
            rated: upload_task.is_some(),

            info,
            result,
            player_name: config.player_name.clone(),
            player_rks,
            autoplay: config.autoplay(),
            use_keyboard: config.use_keyboard,
            speed: config.speed,
            mods: config.mods,
            next: 0,

            upload_fn,
            upload_task,
            record_data,
            best_record,
            detail_mode: false,

            btn_retry: RectButton::new(),
            btn_proceed: RectButton::new(),
            btn_detail: RectButton::new(),

            tr_start: f32::NAN,

            avg_fps,
        })
    }
}

thread_local! {
    static RE_UPLOAD: RefCell<bool> = RefCell::default();
}

impl Scene for EndingScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        tm.seek_to(0.0);
        self.target = target;
        self.bgm.play()?;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.bgm.pause()?;
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.bgm.play()?;
        tm.resume();
        Ok(())
    }

    fn touch(&mut self, _tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = _tm.now() as f32;
        if self.btn_retry.touch(touch) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                                self.tr_start = t;
                self.next = 1;
            }
            return Ok(true);
        }
        if self.btn_proceed.touch(touch) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                                self.tr_start = t;
                self.next = 2;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if RE_UPLOAD.with(|it| std::mem::replace(it.borrow_mut().deref_mut(), false)) && self.upload_task.is_none() {
            self.upload_task = self
                .record_data
                .clone()
                .map(|data| ((self.upload_fn.as_ref().unwrap())(data), show_message(tl!("uploading")).handle()));
        }
        if let Some((task, handle)) = &mut self.upload_task {
            if let Some(result) = task.take() {
                handle.cancel();
                match result {
                    Err(err) => {
                        let error = format!("{:?}", err.context(tl!("upload-failed")));
                        Dialog::plain(tl!("upload-failed"), error)
                            .buttons(vec![tl!("upload-cancel").to_string(), tl!("upload-retry").to_string()])
                            .show();
                    }
                    Ok(state) => {
                        self.update_state = Some(state);
                        show_message(tl!("uploaded")).ok();
                    }
                }
                self.upload_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {

        const MAIN_POS_START: f32 = 0.00;
        const MAIN_POS_END: f32 = 3.00;

        const A_POS_START: f32 = 0.00;
        const A_POS_END: f32 = 1.15;
        const A_SCORE_ALPHA_START: f32 = 1.08;
        const A_SCORE_ALPHA_END: f32 = 1.35;
        const A_ICON_SCALE_START: f32 = 2.50;
        const A_ICON_SCALE_END: f32 = 2.90;
        const A_ICON_ALPHA_START: f32 = 2.40;
        const A_ICON_ALPHA_END: f32 = 2.70;

        const B_POS_START: f32 = 0.00;
        const B_POS_END: f32 = 1.60;
        const B_ALPHA_START: f32 = 1.50;
        const B_ALPHA_END: f32 = 1.83;

        const C_POS_START: f32 = 0.00;
        const C_POS_END: f32 = 2.05;
        const C_ALPHA_START: f32 = 1.91;
        const C_ALPHA_END: f32 = 2.25;

        let (
            text_max_combo,
            text_accuracy,
            text_autoplay,
            text_new_best,
            text_arcperfect,
            text_perfect,
            text_good,
            text_bad,
            text_miss,
            text_early,
            text_late,
        ) = {
            (
                "Max Combo",
                "Accuracy",
                "AUTOPLAY",
                "NEW BEST",
                "Perfect+",
                "Perfect",
                "Good",
                "Bad",
                "Miss",
                "E",
                "L",
            )
        };

        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let top = 1. / asp;
        let t = tm.now() as f32;
        let gl = unsafe { get_internal_gl() }.quad_gl;
        let res = &self.result;
        cam.render_target = self.target;
        set_camera(&cam);
            draw_background(*self.background);

        fn ran(t: f32, l: f32, r: f32) -> f32 {
            ((t - l) / (r - l)).clamp(0., 1.)
        }
        fn tran(gl: &mut QuadGl, x: f32) {
            gl.push_model_matrix(Mat4::from_translation(vec3(x * 2., 0., 0.)));
        }

        let p_main = (1. - ran(t, MAIN_POS_START, MAIN_POS_END) + 0.15).powi(10);
        tran(gl, p_main);
        let r = draw_illustration(*self.illustration, -0.372, -0.002, 1.052, 1.22, WHITE, true);
        let main = Rect::new(r.right() - 0.053, r.y, r.w * 0.782, r.h / 2.);
        let slope = PARALLELOGRAM_SLOPE;
        let ratio = 0.2;
        draw_parallelogram_ex(
            Rect::new(r.x, r.y + r.h * (1. - ratio), r.w - r.h * (1. - ratio) * slope, r.h * ratio),
            None,
            Color::default(),
            Color::new(0., 0., 0., 0.7),
            true,
        );
        let p = (r.x + 0.055, r.bottom() - top / 14.5);
        let mw = (r.right() - p.0) * 0.4 - 0.02;
        draw_text_aligned_opt_width(ui, &self.info.level, r.right() - r.h / 7. * 13. * 0.13 - 0.029, r.bottom() - top / 18.5, (1., 1.), 0.40, WHITE, mw);
        draw_text_aligned_opt_width(ui, &self.info.name, p.0, p.1, (0., 1.), 0.92, WHITE, mw);
        gl.pop_model_matrix();

        let dx = 0.07;
        let c = Color::new(0., 0., 0., 1.0);
        let c2 = Color::new(0., 0., 0., 0.5);

        tran(gl, (1. - ran(t, A_POS_START, A_POS_END)).powi(2) + p_main);
        draw_parallelogram(main, None, c2, true);
        {
            let spd = if (self.speed - 1.).abs() <= 1e-4 {
                format!("XHIGROS")
            } else {
                format!("XHIGROS ({:.2}x)", self.speed)
            };
            let text = if self.autoplay {
                format!("XHIGROS ({text_autoplay}) {spd}")
            } else if !self.rated {
                format!("XHIGROS ({spd})")
            } else if let Some(state) = &self.update_state {
                format!(
                    "{spd}  {}",
                    if state.best {
                        format!("{text_new_best} +{:08}", state.improvement)
                    } else {
                        format!(" ")
                    }
                )
            } else {
                "Uploading…".to_owned()
            };

            let icon = &self.icons[icon_index(res.score, res.max_combo == res.num_of_notes)];
            let is_ap_plus = res.score == 10_000_000 + res.num_of_notes;
            let counts = res.counts;
            let is_fr = counts[2] != 0 && counts[3] + counts[4] == 0 && counts[0] + counts[1] + counts[2] == res.num_of_notes;
            let pa = ran(t, A_SCORE_ALPHA_START, A_SCORE_ALPHA_END);
            let r = draw_text_aligned(ui, &text, main.x + dx + 0.01, main.bottom() - 0.040, (0., 1.), 0.34, Color::new(1., 1., 1., pa));
            let score = res.score;
            let score = format!("{:08}", score);
if is_ap_plus {
    let shadow_offset = 0.006;

    draw_text_aligned_opt_width(
        ui,
        &score,
        r.x - 0.012 + 0.004,
        r.y - 0.019 + shadow_offset,
        (0., 1.),
        1.05,
        Color::new(0.25, 0.55, 1.0, pa * 0.9),
        0.4,
    );
}
if is_fr {
    let shadow_offset = 0.006;

    draw_text_aligned_opt_width(
        ui,
        &score,
        r.x - 0.012 + 0.004,
        r.y - 0.019 + shadow_offset,
        (0., 1.),
        1.05,
        Color::new(0.75,0.45,1.00, pa * 0.9),
        0.4,
    );
}

draw_text_aligned_opt_width(
    ui,
    &score,
    r.x - 0.012,
    r.y - 0.019,
    (0., 1.),
    1.05,
    Color::new(1., 1., 1., pa),
    0.4,
);

            let pa = ran(t, A_ICON_ALPHA_START, A_ICON_ALPHA_END);
            let ps = ran(t, A_ICON_SCALE_START, A_ICON_SCALE_END).powi(3);
            let s = main.h * 0.72;
            let ct = (main.right() + 0.015 - main.h * slope - s / 2., r.bottom() + 0.033 - s / 2.);
            let s = s + s * (1. - ps) * 0.3;
            draw_texture_ex(
                **icon,
                ct.0 - s * 0.99 / 2.,
                ct.1 - s * 1.05 / 2.,
                Color::new(1., 1., 1., pa),
                DrawTextureParams {
                    dest_size: Some(vec2(s * 0.99, s * 1.05)),
                    ..Default::default()
                },
            );
        }
        gl.pop_model_matrix();

        tran(gl, (1. - ran(t, B_POS_START, B_POS_END)).powi(2) + p_main);
        let d = r.h / 15.2;
        let pa = ran(t, B_ALPHA_START, B_ALPHA_END);
        let s1 = Rect::new(main.x - d * 4. * slope, main.bottom() + d, main.w - d * 5. * slope, d * 2.8);
        draw_parallelogram(s1, None, c2, true);
        {
            let dy = 0.025;
            let max_combo = res.max_combo.to_string();
            let r = draw_text_aligned(ui, text_max_combo, s1.x + dx - 0.005, s1.bottom() - dy, (0., 1.), 0.31, Color::new(1., 1., 1., pa));
            draw_text_aligned_opt_width(ui, &max_combo, r.x, r.y - 0.006, (0., 1.), 0.65, Color::new(1., 1., 1., pa), 0.3);
            let accuracy = format!("{:.2}%", res.accuracy * 100.);
            let r = draw_text_aligned(ui, text_accuracy, s1.right() - dx + 0.022, s1.bottom() - dy, (1., 1.), 0.31, Color::new(1., 1., 1., pa));
            draw_text_aligned_opt_width(ui, &accuracy, r.right(), r.y - 0.008, (1., 1.), 0.62, Color::new(1., 1., 1., pa), 0.3);
        }
        gl.pop_model_matrix();

        tran(gl, (1. - ran(t, C_POS_START, C_POS_END)).powi(2) + p_main);
        let s2 = Rect::new(s1.x - d * 4. * slope, s1.bottom() + d, s1.w, s1.h);
        draw_parallelogram(s2, None, c2, true);
        {
            let dy = 0.028;
            let dy2 = 0.010;
            let bg = 0.55;
            let sm = 0.21;
            let pa = ran(t, C_ALPHA_START, C_ALPHA_END);
            let draw_count = |ui: &mut Ui, ratio: f32, name: &str, count: u32| {
                let r = draw_text_aligned(ui, name, s2.x + s2.w * ratio, s2.bottom() - dy, (0.5, 1.), sm, Color::new(1., 1., 1., pa));
                let text = count.to_string();
                draw_text_aligned_opt_width(ui, &text, r.center().x, r.y - dy2, (0.5, 1.), bg, Color::new(1., 1., 1., pa), 0.125);
            };
            draw_count(ui, 0.127, text_arcperfect, res.counts[1]);
            draw_count(ui, 0.325, text_perfect, res.counts[0]);
            draw_count(ui, 0.46, text_good, res.counts[2]);
            draw_count(ui, 0.595, text_bad, res.counts[3]);
            draw_count(ui, 0.73, text_miss, res.counts[4]);

            let sm = 0.32;
            let l = s2.x + s2.w * 0.82;
            let rt = s2.x + s2.w * 0.930;
            let cy = s2.center().y;
            let (early, late) = (res.early.to_string(), res.late.to_string());
            let r = draw_text_aligned(ui, text_early, l, cy, (0., 1.), sm, Color::new(1., 1., 1., pa));
            draw_text_aligned_opt_width(ui, &early, rt, r.bottom(), (1., 1.), sm, Color::new(1., 1., 1., pa), 0.1);
            let r = draw_text_aligned(ui, text_late, l, cy + dy2 / 2.3, (0., 0.), sm, Color::new(1., 1., 1., pa));
            draw_text_aligned_opt_width(ui, &late, rt, r.y, (1., 0.), sm, Color::new(1., 1., 1., pa), 0.1);
        }
        gl.pop_model_matrix();

        let dy = 0.010;
        let w = 0.202;
        let p = (1. - ran(t, 1.2, 2.4)).powi(7);
        let p2 = (1. - ran(t, 1.35, 2.4)).powi(5);
        let h = 0.117;
        let s = 0.10;
        let hs = h * 0.28;
        let params = DrawTextureParams {
            dest_size: Some(vec2(hs * 2., hs * 2.)),
            ..Default::default()
        };
        tran(gl, -p * 0.1);
        let r = Rect::new(-1. - h * slope, -top + dy, w, h);
        draw_parallelogram(r, None, c, true);
        draw_parallelogram(Rect::new(r.x + r.w * (1. - s), r.y, r.w * s, r.h), None, WHITE, false);
        let ct = r.center();
        draw_texture_ex(*self.icon_retry, ct.x - hs * 0.9, ct.y - hs, WHITE, params.clone());
        gl.pop_model_matrix();
        if p <= 0. {
                self.btn_retry.set(ui, r);
        }

        tran(gl, p2 * 0.1);
        let r = Rect::new(1. + h * slope - w, top - dy - h, w, h);
        draw_parallelogram(r, None, c, true);
        draw_parallelogram(Rect::new(r.x, r.y, r.w * s, r.h), None, WHITE, false);
        let ct = r.center();
        draw_texture_ex(*self.icon_proceed, ct.x - hs * 0.8 - r.w * s / 2., ct.y - hs, WHITE, params);
        gl.pop_model_matrix();
        if p <= 0. {
            self.btn_proceed.set(ui, r);
        }

        let alpha = ran(t, 1.25, 1.75);
        let main = Rect::new(1. - 0.27, -top + dy * 3.2, 0.35, 0.11);
        draw_parallelogram(main, None, Color::new(0., 0., 0., c.a * alpha), false);
        let sub = Rect::new(1. - 0.125, main.center().y + 0.015, 0.12, 0.03);
        let color = Color::new(1., 1., 1., alpha);
        draw_parallelogram(sub, None, color, false);
        draw_text_aligned_opt_width(
            ui,

        &self
    .player_rks
    .map(|rks| format!("{:.2}", rks))
    .unwrap_or_default()

            ,
            sub.center().x,
            sub.center().y - 0.002,
            (0.5, 0.5),
            0.37,
            Color::new(0., 0., 0., alpha),
            0.10
        );
        let r = draw_illustration(*self.player, 1. - 0.21, main.center().y, 0.12 / (0.076 * 7.), 0.12 / (0.076 * 7.), color, true);
        let mut text = ui.text(&self.player_name).pos(r.x - 0.015, r.center().y - 0.002).anchor(1., 0.5).size(0.54).color(color);
        let text_rect = text.measure();
        draw_parallelogram(
            Rect::new(text_rect.x - main.h * slope - 0.02, main.y, r.x - text_rect.x + main.h * slope * 2. + 0.021, main.h),
            None,
            Color::new(0., 0., 0., c.a * alpha),
            false,
        );
        text.draw();

        let ct = (1. - 0.1 + 0.043, main.center().y - 0.034 + 0.02);
        let r = Rect::new(ct.0 - w / 2., ct.1 - h / 2., w, h);
        let _ct = r.center();
        let mut text_size = 0.46;
        let max_width = 0.05;
        let text_width = text.measure().w;
        if text_width > max_width {
            text_size *= max_width / text_width
        }
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if self.next != 0 {
            let _ = self.bgm.pause();
        }
        match self.next {
            0 => NextScene::None,
            1 => NextScene::Pop,
            2 => {
                if let Some(rec) = &self.best_record {
                    NextScene::PopNWithResult(2, Box::new(rec.clone()))
                } else {
                    NextScene::PopN(2)
                }
            }
            _ => unreachable!(),
        }
    }
}
