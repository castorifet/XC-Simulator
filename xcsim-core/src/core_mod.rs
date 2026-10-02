
pub use macroquad::color::Color;

pub const NOTE_WIDTH_RATIO_BASE: f32 = 0.13175016;
pub const HEIGHT_RATIO: f32 = 0.83175;

pub const EPS: f32 = 1e-5;

pub type Point = nalgebra::Point2<f32>;
pub type Vector = nalgebra::Vector2<f32>;
pub type Matrix = nalgebra::Matrix3<f32>;

#[path = "core_mod/anim_core.rs"] mod anim;
pub use anim::{Anim, AnimFloat, AnimVector, Keyframe};

#[path = "core_mod/chart_core.rs"] mod chart;
pub use chart::{Chart, ChartExtra, ChartSettings, HitSoundMap};

#[path = "core_mod/effect_core.rs"] mod effect;
pub use effect::{Effect, Uniform};

#[path = "core_mod/line_core.rs"] mod line;
pub use line::{GifFrames, JudgeLine, JudgeLineCache, JudgeLineKind, UIElement};

#[path = "core_mod/note_core.rs"] mod note;
use macroquad::prelude::set_pc_assets_folder;
pub use note::{BadNote, HitSound, Note, NoteKind, RenderConfig};

#[path = "core_mod/object_core.rs"] mod object;
pub use object::{CtrlObject, Object};

#[path = "core_mod/render_core.rs"] mod render;
pub use render::{copy_fbo, internal_id, MSRenderTarget};

#[path = "core_mod/resource_core.rs"] mod resource;
pub use resource::{NoteStyle, ParticleEmitter, ResPackInfo, Resource, ResourcePack, BUFFER_SIZE, DPI_VALUE};

#[path = "core_mod/smooth_core.rs"] mod smooth;
pub use smooth::Smooth;

#[path = "core_mod/tween_core.rs"] mod tween;
pub use tween::{
    easing_from, BezierTween, ClampedTween, GeneralIntTween, IntClampedTween, IntStaticTween, StaticTween, TweenFunction, TweenId, TweenMajor,
    TweenMinor, Tweenable, TWEEN_FUNCTIONS,
};

#[cfg(feature = "video")]
#[path = "core_mod/video_core.rs"] mod video;
#[cfg(feature = "video")]
pub use xcsim_core_avc::demux_audio;
#[cfg(feature = "video")]
pub use video::{Video, VideoAttach};

use crate::ui::TextPainter;
use std::cell::RefCell;

thread_local! {
    pub static PGR_FONT: RefCell<Option<TextPainter>> = RefCell::default();
    pub static BOLD_FONT: RefCell<Option<TextPainter>> = RefCell::default();
}

pub fn init_assets() {
    #[cfg(not(target_env = "ohos"))]
    if let Ok(mut exe) = std::env::current_exe() {
        while exe.pop() {
            if exe.join("assets").exists() {
                std::env::set_current_dir(exe).unwrap();
                break;
            }
        }
    }
    #[cfg(target_env = "ohos")]
    let _ = std::env::set_current_dir("/data/storage/el1/bundle/entry/resources/resfile/");
    set_pc_assets_folder("assets");
}

#[derive(serde::Deserialize)]

pub struct Triple(i32, u32, u32);
impl Default for Triple {
    fn default() -> Self {
        Self(0, 0, 1)
    }
}

impl Triple {
    pub fn beats(&self) -> f32 {
        self.0 as f32 + self.1 as f32 / self.2 as f32
    }
}

#[derive(Default)]
pub struct BpmList {

    elements: Vec<(f32, f32, f32)>,

    cursor: usize,
}

impl BpmList {

    pub fn new(ranges: Vec<(f32, f32)>) -> Self {
        let mut elements = Vec::new();
        let mut time = 0.0;
        let mut last_beats = 0.0;
        let mut last_bpm: Option<f32> = None;
        for (now_beats, bpm) in ranges {
            if let Some(bpm) = last_bpm {
                time += (now_beats - last_beats) * (60. / bpm);
            }
            last_beats = now_beats;
            last_bpm = Some(bpm);
            elements.push((now_beats, time, bpm));
        }
        BpmList { elements, cursor: 0 }
    }

    pub fn time_beats(&mut self, beats: f32) -> f32 {
        while let Some(kf) = self.elements.get(self.cursor + 1) {
            if kf.0 > beats {
                break;
            }
            self.cursor += 1;
        }
        while self.cursor != 0 && self.elements[self.cursor].0 > beats {
            self.cursor -= 1;
        }
        let (start_beats, time, bpm) = &self.elements[self.cursor];
        time + (beats - start_beats) * (60. / bpm)
    }

    pub fn time(&mut self, triple: &Triple) -> f32 {
        self.time_beats(triple.beats())
    }

    pub fn beat(&mut self, time: f32) -> f32 {
        while let Some(kf) = self.elements.get(self.cursor + 1) {
            if kf.1 > time {
                break;
            }
            self.cursor += 1;
        }
        while self.cursor != 0 && self.elements[self.cursor].1 > time {
            self.cursor -= 1;
        }
        let (beats, start_time, bpm) = &self.elements[self.cursor];
        beats + (time - start_time) / (60. / bpm)
    }
}
