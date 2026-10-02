use super::{draw_background, ending::RecordUpdateState, game::GameMode, GameScene, NextScene, Scene};
use crate::{
    config::Config,
    core::Resource,
    ext::{poll_future,draw_parallelogram,draw_illustration,draw_text_aligned, draw_text_aligned_opt, draw_text_aligned_opt_width, LocalTask, SafeTexture, BLACK_TEXTURE},
    fs::FileSystem,
    info::ChartInfo,
    judge::Judge,
    task::Task,
    scene::game::SimpleRecord,
    time::TimeManager,
    ui::Ui,
};
use ::rand::{seq::SliceRandom, thread_rng};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use regex::Regex;
use std::sync::Arc;
use tracing::warn;

const BEFORE_TIME: f32 = 1.;
const TRANSITION_TIME: f32 = 1.4;
const WAIT_TIME: f32 = 0.4;
const FADE_IN_TIME: f32 = 0.6;

const DEFAULT_WHITE_FADE_IN: f32 = 0.5;

const NAME_SLIDE_IN_TIME: f32 = 0.8;

const NAME_SLIDE_IN_DIST: f32 = 1.6;

pub type UploadFn = Arc<dyn Fn(Vec<u8>) -> Task<Result<RecordUpdateState>>>;
pub type UpdateFn = Box<dyn FnMut(f32, &mut Resource, &mut Judge)>;
pub type SaveFn = Box<dyn Fn(SimpleRecord) -> Result<()>>;
pub struct BasicPlayer {
    pub avatar: Option<SafeTexture>,
    pub id: i32,
    pub rks: f32,
    pub historic_best: u32,
}

pub struct LoadingScene {
    info: ChartInfo,
    background: SafeTexture,
    illustration: SafeTexture,
    pub load_task: LocalTask<Result<GameScene>>,
    next_scene: Option<NextScene>,
    finish_time: f32,
    target: Option<RenderTarget>,
    charter: String,

    theme_color: Color,
    use_black: bool,

    pub white_fade_in: f32,
    enter_time: f32,

    pub pause_disabled: bool,
}

impl LoadingScene {

    pub fn set_white_fade_in(&mut self, duration: f32) {
        self.white_fade_in = duration;
    }

    pub fn set_pause_disabled(&mut self, disabled: bool) {
        self.pause_disabled = disabled;
    }
}

impl LoadingScene {
    pub const TOTAL_TIME: f32 = BEFORE_TIME + TRANSITION_TIME + WAIT_TIME;

    pub async fn load(fs: &mut dyn FileSystem, path: &str) -> Result<(SafeTexture, SafeTexture, Color)> {
        let image = image::load_from_memory(&fs.load_file(path).await?).context("Failed to decode image")?;
        let (w, h) = (image.width(), image.height());
        let size = w as usize * h as usize;

        let mut blurred_rgb = image.to_rgb8();
        let color = color_thief::get_palette(&blurred_rgb, color_thief::ColorFormat::Rgb, 10, 2)?[0];
        let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute(blurred_rgb.as_mut_ptr()), size, size) };
        fastblur::gaussian_blur(&mut vec, w as _, h as _, 50.);
        std::mem::forget(vec);
        let mut blurred = Vec::with_capacity(size * 4);
        for input in blurred_rgb.chunks_exact(3) {
            blurred.extend_from_slice(input);
            blurred.push(255);
        }
        Ok((
            Texture2D::from_rgba8(w as _, h as _, &image.into_rgba8()).into(),
            Texture2D::from_image(&Image {
                width: w as _,
                height: h as _,
                bytes: blurred,
            })
            .into(),
            Color::from_rgba(color.r, color.g, color.b, 255),
        ))
    }

    pub async fn new(
        mode: GameMode,
        mut info: ChartInfo,
        config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
        save_fn: Option<SaveFn>,
        preloaded: Option<(SafeTexture, SafeTexture, Color)>,
    ) -> Result<Self> {
        let (background, theme_color) = match preloaded {
            Some((ill, bg, color)) => (Some((ill, bg)), color),
            None => match Self::load(fs.as_mut(), &info.illustration).await {
                Ok((ill, bg, color)) => (Some((ill, bg)), color),
                Err(err) => {
                    warn!("failed to load background: {err:?}");
                    (None, WHITE)
                }
            },
        };
        let use_black = (theme_color.r * 0.299 + theme_color.g * 0.587 + theme_color.b * 0.114) > 186. / 255.;
        let (illustration, background) = background.unwrap_or_else(|| (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()));
        if info.tip.is_none() {
            info.tip = Some(crate::config::TIPS.choose(&mut thread_rng()).unwrap().to_owned());
        }
        let future = Box::pin(GameScene::new(mode, info.clone(), config, fs, player, background.clone(), illustration.clone(), upload_fn, update_fn, save_fn));
        let charter = Regex::new(r"\[!:[0-9]+:([^:]*)\]").unwrap().replace_all(&info.charter, "$1").to_string();

        Ok(Self {
            info,
            background,
            illustration,
            load_task: Some(future),
            next_scene: None,
            finish_time: f32::INFINITY,
            target: None,
            charter,

            theme_color,
            use_black,

            white_fade_in: DEFAULT_WHITE_FADE_IN,
            enter_time: f32::NAN,
            pause_disabled: false,
        })
    }
}

impl Scene for LoadingScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.target = target;
        if self.enter_time.is_nan() {
            self.enter_time = tm.now() as f32;
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(future) = self.load_task.as_mut() {
            loop {
                match poll_future(future.as_mut()) {
                    None => {
                        if self.target.is_none() {
                            break;
                        }
                        std::thread::yield_now();
                    }
                    Some(game_scene) => {
                        self.load_task = None;
                        let pause_disabled = self.pause_disabled;
                        self.next_scene = Some(game_scene.map_or_else(
                            |e| NextScene::PopWithResult(Box::new(e)),
                            |mut it| {
                                it.set_pause_disabled(pause_disabled);
                                NextScene::Replace(Box::new(it))
                            },
                        ));
                        self.finish_time = tm.now() as f32 + BEFORE_TIME;
                        break;
                    }
                }
            }
        }
        Ok(())
    }

fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
    let cam = ui.camera();
    let asp = -cam.zoom.y;
    let top = 1. / asp;
         let now = tm.now() as f32;
        let intern = unsafe { get_internal_gl() };
        let gl = intern.quad_gl;
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            render_target: self.target,
            ..Default::default()
        });
        draw_background(*self.background);
        let dx = if now > self.finish_time {
            let p = ((now - self.finish_time) / TRANSITION_TIME).min(1.);
            p.powi(2) * 3. + p.powi(5) * 11.
        } else {
            0.
        };
        if dx != 0. {
            gl.push_model_matrix(Mat4::from_translation(vec3(dx, 0., 0.)));
        }
        let vo = -top / 10.;
        let voi = -top / 8.5;
        let r = draw_illustration(*self.illustration, 0.380, voi, 1.03, 1.0, WHITE, false);
        let h = r.h / 3.55;
        let main: Rect = Rect::new(-0.87, vo - h / 2. - top / 10., 0.768, h);
        draw_parallelogram(main, None, Color::new(0., 0., 0., 0.6), false);
        let p1 = (main.x + main.w * 0.085, main.y + main.h * 0.35 + 0.025);
        let p2 = (main.x + main.w * 0.09, main.y + main.h * 0.74 - 0.0125);

        let (name_dx, name_alpha) = if self.enter_time.is_nan() {
            (0., 1.)
        } else {
            let elapsed = (now - self.enter_time).max(0.);
            let p = (elapsed / NAME_SLIDE_IN_TIME).clamp(0., 1.);
            let p_eased = 1. - (1. - p).powi(3);
            (-NAME_SLIDE_IN_DIST * (1. - p_eased), p_eased)
        };
        let name_color = Color { a: name_alpha, ..WHITE };

        draw_text_aligned_opt(ui, &self.info.name, p1.0 + name_dx, p1.1, (0., 1.0), 0.73, name_color, main.w * 0.65, main.h * 0.5);
        draw_text_aligned_opt(ui, &self.info.composer, p2.0 + name_dx, p2.1, (0., 0.0), 0.363, name_color, main.w * 0.60, main.h * 0.25);

        let ext = 0.04;
        let sub = Rect::new(main.x + main.w * 0.724, main.y - main.h * ext, main.w * 0.25, main.h * (1. + ext * 2.));
        let mut ct = sub.center();
        ct.x += sub.w * 0.01;
        ct.y += sub.h * 0.05;
        draw_parallelogram(sub, None, WHITE, true);

            let first_str = Regex::new(r"[0-9?]+").unwrap();
            let last_str = Regex::new(r"[0-9?.]+").unwrap();
            draw_text_aligned_opt_width(ui, self.info.level
                .split_whitespace()
                .rev()
                .nth(0)

                .and_then(|word| { first_str.find(word).map(|m| &word[m.start()..]) })
                .and_then(|word| { last_str.find(word).map(|m| &word[..m.end()]) })

                .unwrap_or(

                    "?"
                ),
                ct.x, ct.y + sub.h * 0.05, (0.5, 1.), 0.90, BLACK, main.w * 0.18
            );

        draw_text_aligned_opt_width(ui, self.info.level
            .split_whitespace()
            .next()
            .unwrap_or("?")
            , ct.x, ct.y + sub.h * 0.09, (0.5, 0.), 0.30, BLACK, main.w * 0.16
        );
        let w = 0.031;
        let h = 0.030;
        let (text_chart, text_illustration) = {("Chart", "Illustration")};

        let t = draw_text_aligned(ui, text_chart, main.x + main.w / 6.1 + name_dx, main.y + main.h * 1.32, (0., 0.), 0.253, name_color);
        let t = draw_text_aligned_opt_width(ui, &self.charter, t.x, t.y + top / 22., (0., 0.), 0.415, name_color, 0.58);

        let t = draw_text_aligned(ui, text_illustration, t.x - w, t.y + t.h + h, (0., 0.), 0.253, name_color);
        draw_text_aligned_opt_width(ui, &self.info.illustrator, t.x - 0.002, t.y + top / 22., (0., 0.), 0.415, name_color, 0.58);
        let text_tip = self.info.tip.as_ref().unwrap();
        draw_text_aligned_opt_width(ui, &text_tip, -0.895 + name_dx, top * 0.88, (0., 1.), 0.47, name_color, 1.55);
        let text_loading = {"Loading..."};
        let t = draw_text_aligned(ui, &text_loading, 0.865, top * 0.865, (1., 1.), 0.41, WHITE);
        let we = 0.19;
        let he = 0.35;
        let r = Rect::new(t.x - t.w * we, t.y - t.h * he, t.w * (1. + we * 2.2), t.h * (1. + he * 2.2));

        let p = 0.6;
        let s = 0.2;
        let t = ((now - 0.3).max(0.) % (p * 2. + s)) / p;
        let st = (t - 1.).clamp(0., 1.).powi(3);
        let en = 1. - (1. - t.min(1.)).powi(3);

        let mut r = Rect::new(r.x + r.w * st, r.y, r.w * (en - st), r.h);
        ui.fill_rect(r, WHITE);
        r.x += dx;
        draw_text_aligned(ui, text_loading, 0.865, top * 0.865, (1., 1.), 0.41, BLACK);

        if dx != 0. {
            gl.pop_model_matrix();
        }

        if self.white_fade_in > 0. && !self.enter_time.is_nan() {
            let elapsed = (now - self.enter_time).max(0.);
            if elapsed < self.white_fade_in {
                let alpha = 1. - (elapsed / self.white_fade_in).clamp(0., 1.);
                let r = Rect::new(-1., -top, 2., top * 2.);
                ui.fill_rect(r, Color::new(1., 1., 1., alpha));
            }
        }
        Ok(())

    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if matches!(self.next_scene, Some(NextScene::PopWithResult(_))) {
            return self.next_scene.take().unwrap();
        }
        if tm.now() as f32 > self.finish_time + TRANSITION_TIME + WAIT_TIME {
            if let Some(scene) = self.next_scene.take() {
                return scene;
            }
        }
        NextScene::None
    }
}
