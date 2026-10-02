#[path = "page_app/coll_page.rs"] pub mod coll;

#[path = "page_app/favorites_page.rs"] pub mod favorites;
pub use favorites::FavoritesPage;

#[path = "page_app/home_page.rs"] mod home;
pub use home::HomePage;

#[path = "page_app/library_page.rs"] mod library;
pub use library::{ExportInfo, LibraryPage, CHOOSE_COVER, CHOSEN_COVER, FAV_UPDATED};

#[path = "page_app/message_page.rs"] mod message;
pub use message::MessagePage;

#[path = "page_app/offset_page.rs"] mod offset;
pub use offset::OffsetPage;

#[path = "page_app/respack_page.rs"] mod respack;
pub use respack::{ResPackItem, ResPackPage};

#[path = "page_app/settings_page.rs"] mod settings;
pub use settings::SettingsPage;
use tokio::sync::Notify;

use crate::{
    client::{ChartRef, File},
    data::BriefChartInfo,
    dir, get_data,
    images::Images,
    scene::fs_from_path,
};
use anyhow::Result;
use image::DynamicImage;
use macroquad::prelude::*;
use xcsim_core::{
    core::{Resource, BOLD_FONT},
    ext::{semi_black, semi_white, SafeTexture, ScaleType, BLACK_TEXTURE},
    fs,
    scene::{NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{FontArc, IntoShading, Shading, TextPainter, Ui},
};
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    ops::DerefMut,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing::warn;

pub fn thumbnail_path(path: &str) -> Result<PathBuf> {
    Ok(format!("{}/{}", dir::cache_image_local()?, path.replace('/', "_")).into())
}

pub fn illustration_task(notify: Arc<Notify>, path: String, full: bool) -> Task<Result<(DynamicImage, Option<DynamicImage>)>> {
    Task::new(async move {
        notify.notified().await;
        let mut fs = fs_from_path(&path)?;
        let info = fs::load_info(fs.deref_mut()).await?;
        let mut img = None;
        let thumbnail = Images::local_or_else(thumbnail_path(&path)?, async {
            let image = image::load_from_memory(&fs.load_file(&info.illustration).await?)?;
            let thumbnail = Images::thumbnail(&image);
            img = Some(image);
            Ok(thumbnail)
        })
        .await?;
        if full {
            if img.is_none() {
                img = Some(image::load_from_memory(&fs.load_file(&info.illustration).await?)?);
            }
        } else {
            img = None;
        }
        Ok((thumbnail, img))
    })
}

pub fn local_illustration(path: String, def: SafeTexture, full: bool) -> Illustration {
    let notify = Arc::new(Notify::new());
    Illustration {
        texture: (def.clone(), def),
        notify: Arc::clone(&notify),
        task: Some(illustration_task(notify, path, full)),
        loaded: Arc::default(),
        load_time: f32::NAN,
    }
}

pub fn load_local() -> Vec<ChartItem> {
    let tex = BLACK_TEXTURE.clone();
    get_data()
        .charts
        .iter()
        .map(|it| ChartItem {
            info: it.info.clone(),
            local_path: Some(it.local_path.clone()),
            illu: local_illustration(it.local_path.clone(), tex.clone(), false),
            chart_type: ChartType::Imported,
        })
        .collect()
}

#[derive(Clone)]
pub struct CachedChartEntry {
    pub name: String,
    pub info: xcsim_core::info::ChartInfo,
}

#[derive(Default, Clone)]
pub struct AssetChartsCache {
    pub special: Vec<CachedChartEntry>,
    pub chapters: Vec<ChapInfo>,
    pub chap_charts: std::collections::HashMap<String, Vec<CachedChartEntry>>,
}

static ASSET_CHARTS_CACHE: std::sync::OnceLock<AssetChartsCache> = std::sync::OnceLock::new();

fn get_cache() -> AssetChartsCache {
    ASSET_CHARTS_CACHE.get().cloned().unwrap_or_default()
}

async fn read_manifest_lines(path: &str) -> Option<Vec<String>> {
    let bytes = macroquad::file::load_file(path).await.ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    Some(
        text.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_owned())
            .collect(),
    )
}

async fn load_chart_info(path: &str) -> Option<xcsim_core::info::ChartInfo> {
    let bytes = macroquad::file::load_file(path).await.ok()?;
    serde_yaml::from_slice(&bytes).ok()
}

async fn load_chap_display_name(path: &str) -> Option<String> {
    let bytes = macroquad::file::load_file(path).await.ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("name") {
                let n = v.trim();
                if !n.is_empty() {
                    return Some(n.to_owned());
                }
            }
        }
    }
    None
}

fn fs_list_subdirs(path: &str) -> Vec<String> {
    let dir = std::path::Path::new("assets").join(path);
    let Ok(it) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    it.flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect()
}

pub async fn init_asset_charts() {
    let mut cache = AssetChartsCache::default();

    let mut special_names = read_manifest_lines("charts/special/manifest.txt").await.unwrap_or_default();
    if special_names.is_empty() {
        special_names = fs_list_subdirs("charts/special");
    }
    for name in special_names {
        let info_path = format!("charts/special/{name}/info.yml");
        match load_chart_info(&info_path).await {
            Some(info) => cache.special.push(CachedChartEntry { name, info }),
            None => warn!(?info_path, "failed to load BM Default chart info"),
        }
    }

    let mut chapter_ids = read_manifest_lines("charts/chap/manifest.txt").await.unwrap_or_default();
    if chapter_ids.is_empty() {
        chapter_ids = fs_list_subdirs("charts/chap");
    }
    for chap_id in &chapter_ids {
        let display_name = load_chap_display_name(&format!("charts/chap/{chap_id}/info.txt"))
            .await
            .unwrap_or_else(|| chap_id.clone());
        cache.chapters.push(ChapInfo {
            id: chap_id.clone(),
            name: display_name,
        });

        let mut chart_names = read_manifest_lines(&format!("charts/chap/{chap_id}/manifest.txt"))
            .await
            .unwrap_or_default();
        if chart_names.is_empty() {
            chart_names = fs_list_subdirs(&format!("charts/chap/{chap_id}"));
        }
        let mut entries = Vec::new();
        for chart_name in chart_names {
            let info_path = format!("charts/chap/{chap_id}/{chart_name}/info.yml");
            match load_chart_info(&info_path).await {
                Some(info) => entries.push(CachedChartEntry { name: chart_name, info }),
                None => warn!(?info_path, "failed to load chapter chart info"),
            }
        }
        cache.chap_charts.insert(chap_id.clone(), entries);
    }
    cache.chapters.sort_by(|a, b| a.name.cmp(&b.name));

    let _ = ASSET_CHARTS_CACHE.set(cache);
}

pub fn load_special_charts() -> Vec<ChartItem> {
    let tex = BLACK_TEXTURE.clone();
    get_cache()
        .special
        .into_iter()
        .map(|entry| {
            let local_path = format!("@special/{}", entry.name);
            ChartItem {
                info: BriefChartInfo { id: None, ..entry.info.into() },
                local_path: Some(local_path.clone()),
                illu: local_illustration(local_path, tex.clone(), false),
                chart_type: ChartType::Integrated,
            }
        })
        .collect()
}

#[derive(Clone)]
pub struct ChapInfo {
    pub id: String,
    pub name: String,
}

pub fn list_chap_chapters() -> Vec<ChapInfo> {
    get_cache().chapters
}

pub fn load_chap_charts(chapter: &str) -> Vec<ChartItem> {
    let tex = BLACK_TEXTURE.clone();
    let cache = get_cache();
    let entries = cache.chap_charts.get(chapter).cloned().unwrap_or_default();
    entries
        .into_iter()
        .map(|entry| {
            let local_path = format!("@chap/{chapter}/{}", entry.name);
            ChartItem {
                info: BriefChartInfo { id: None, ..entry.info.into() },
                local_path: Some(local_path.clone()),
                illu: local_illustration(local_path, tex.clone(), false),
                chart_type: ChartType::Integrated,
            }
        })
        .collect()
}

type IllustrationTask = Task<Result<(DynamicImage, Option<DynamicImage>)>>;

#[derive(Clone)]
pub struct Illustration {
    pub texture: (SafeTexture, SafeTexture),
    pub notify: Arc<Notify>,
    pub task: Option<IllustrationTask>,
    pub loaded: Arc<Mutex<Option<(SafeTexture, SafeTexture)>>>,
    pub load_time: f32,
}

impl Illustration {
    const TIME: f32 = 0.4;

    pub fn from_file(file: File) -> Self {
        let notify = Arc::default();
        Self {
            texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
            notify: Arc::clone(&notify),
            task: Some(Task::new(async move {
                notify.notified().await;
                Ok((file.load_image().await?, None))
            })),
            loaded: Arc::default(),
            load_time: f32::NAN,
        }
    }

    pub fn from_file_thumbnail(file: File) -> Self {
        let notify = Arc::default();
        Self {
            texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
            notify: Arc::clone(&notify),
            task: Some(Task::new(async move {
                notify.notified().await;
                Ok((file.load_thumbnail().await?, None))
            })),
            loaded: Arc::default(),
            load_time: f32::NAN,
        }
    }

    pub fn from_done(tex: SafeTexture) -> Self {
        Self {
            texture: (tex.clone(), tex),
            notify: Arc::default(),
            task: None,
            loaded: Arc::default(),
            load_time: f32::NAN,
        }
    }

    pub fn notify(&self) {
        self.notify.notify_one();
    }

    pub fn settle(&mut self, t: f32) {
        if let Some(task) = &mut self.task {
            if let Some(illu) = task.take() {
                match illu {
                    Err(err) => {
                        warn!(?err, "failed to load illustration");
                    }
                    Ok(illu) => {
                        self.texture = Images::into_texture(illu);
                    }
                };
                *self.loaded.lock().unwrap() = Some(self.texture.clone());
                self.task = None;
                self.load_time = t;
            } else if let Some(loaded) = self.loaded.lock().unwrap().clone() {
                self.texture = loaded;
                self.load_time = t;
                self.task = None;
            }
        } else if self.load_time.is_nan() {
            self.load_time = t;
        }
    }

    pub fn alpha(&self, t: f32) -> f32 {
        if self.load_time.is_nan() {
            0.
        } else if get_data().prefer_reduced_motion {
            1.
        } else {
            ((t - self.load_time) / Self::TIME).min(1.)
        }
    }

    pub fn shading(&self, r: Rect, t: f32) -> impl Shading {
        (*self.texture.0, r, ScaleType::CropCenter, semi_white(self.alpha(t))).into_shading()
    }
}

#[derive(Clone)]
pub struct ChartItem {
    pub info: BriefChartInfo,
    pub local_path: Option<String>,
    pub illu: Illustration,
    pub chart_type: ChartType,
}
impl ChartItem {
    pub fn to_ref(&self) -> ChartRef {
        if let Some(local) = &self.local_path {
            ChartRef::Local(local.clone())
        } else if let Some(id) = self.info.id {
            ChartRef::Online(id, None)
        } else {
            panic!("chart item has neither id nor local path");
        }
    }
}

#[derive(Clone, Copy)]
pub enum ChartType {
    Downloaded,
    Imported,
    Integrated,
}

pub struct Fader {
    pub distance: f32,
    start_time: f32,
    pub time: f32,
    index: usize,
    back: bool,
    pub sub: bool,
}

impl Fader {
    const DELTA: f32 = 0.04;

    pub fn new() -> Self {
        Self {
            distance: 0.12,
            start_time: f32::NAN,
            time: 0.33,
            index: 0,
            back: false,
            sub: false,
        }
    }

    #[inline]
    pub fn with_time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }

    #[inline]
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.distance = distance;
        self
    }

    #[inline]
    pub fn reset(&mut self) {
        self.index = 0;
    }

    #[inline]
    pub fn sub(&mut self, t: f32) {
        self.start_time = t;
        self.back = false;
    }

    #[inline]
    pub fn for_sub<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.sub = true;
        let res = f(self);
        self.sub = false;
        res
    }

    #[inline]
    pub fn back(&mut self, t: f32) {
        self.start_time = t;
        self.back = true;
    }

    pub fn progress_scaled(&self, t: f32, scale: f32) -> f32 {
        if self.start_time.is_nan() {
            0.
        } else {
            let linear = if get_data().prefer_reduced_motion {
                1.
            } else {
                ((t - self.start_time) / self.time * scale).clamp(0., 1.)
            };

            let exp_factor: f32 = 7.0;
            let denom = 1.0 - (-exp_factor).exp();
            let s = (1.0 - (-exp_factor * linear).exp()) / denom;
            let p = if self.back { 1.0 - s } else { s };
            if self.sub { 1. - p } else { -p }
        }
    }

    pub fn progress(&self, t: f32) -> f32 {
        self.progress_scaled(t, 1.)
    }

    pub fn roll_back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn render<R>(&mut self, ui: &mut Ui, t: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        let p = self.progress(t - self.index as f32 * Self::DELTA);
        let (dy, alpha) = (p * self.distance, 1. - p.abs());
        self.index += 1;
        ui.scope(|ui| {
            ui.dy(dy);
            ui.alpha(alpha, f)
        })
    }

    #[inline]
    pub fn transiting(&self) -> bool {
        !self.start_time.is_nan()
    }

    pub fn done(&mut self, t: f32) -> Option<bool> {
        if !self.start_time.is_nan() && (t - self.start_time > self.time || get_data().prefer_reduced_motion) {
            self.start_time = f32::NAN;
            Some(self.back)
        } else {
            None
        }
    }

    pub fn render_title(&mut self, ui: &mut Ui, t: f32, s: &str) {
        let tp = ui.back_rect().center().y;
        let h = ui.text("L").size(1.2).no_baseline().measure().h;
        ui.scissor(Rect::new(-1., tp - h / 2., 2., h), |ui| {
            let p = self.progress_scaled(t, 1.6);
            let tp = tp + h * p - h / 2.;
            let mut x = -0.87;
            if s == "PHIRA" {
                x -= ui.back_rect().w;
            }
            for c in s.chars() {
                x += ui
                    .text(c.to_string())
                    .pos(x, tp)
                    .anchor(0., 0.)
                    .size(1.2)
                    .color(WHITE)
                    .draw()
                    .w
                    + 0.012;
            }
            if s == "PHIRA" {
                ui.text(concat!('v', env!("CARGO_PKG_VERSION")))
                    .pos(x + 0.01, tp + h - 0.027)
                    .anchor(0., 1.)
                    .color(semi_white(0.4))
                    .size(0.5)
                    .draw();
            }
        });
    }
}

pub struct SFader {
    time: f32,
    next_scene: Option<NextScene>,
}

impl SFader {
    const TIME: f32 = 0.35;

    pub fn new() -> Self {
        Self {
            time: f32::NAN,
            next_scene: None,
        }
    }

    pub fn transiting(&self) -> bool {
        !self.time.is_nan()
    }

    pub fn goto(&mut self, t: f32, scene: impl Scene + 'static) {
        self.time = t;
        self.next_scene = Some(NextScene::Overlay(Box::new(scene)));
    }

    pub fn next(&mut self, t: f32, next: NextScene) {
        self.time = t;
        self.next_scene = Some(next);
    }

    pub fn enter(&mut self, t: f32) {
        self.time = t;
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        if self.time.is_nan() {
            return;
        }
        let lin = if get_data().prefer_reduced_motion {
            1.
        } else {
            ((t - self.time) / Self::TIME).min(1.)
        };
        let p = 1. - (1. - lin).powi(3);
        if lin >= 1. && self.next_scene.is_none() {
            self.time = f32::NAN;
        } else {
            ui.fill_rect(ui.screen_rect(), semi_black(if self.next_scene.is_some() { p } else { 1. - p }));
        }
    }

    pub fn next_scene(&mut self, t: f32) -> Option<NextScene> {
        if t >= self.time + Self::TIME {
            self.next_scene.take()
        } else {
            None
        }
    }
}

pub struct SharedState {
    pub t: f32,
    pub rt: f32,
    pub fader: Fader,
    pub charts_local: Vec<ChartItem>,

    pub icons: [SafeTexture; 8],
}

thread_local! {
    static FALLBACK: RefCell<Option<FontArc>> = RefCell::default();
    static CJK_FALLBACK: RefCell<Option<FontArc>> = RefCell::default();
    pub static BOLD_FONT_CKSUM: RefCell<Option<String>> = RefCell::default();
}

pub fn set_cjk_fallback(font: Option<FontArc>) {
    CJK_FALLBACK.with(|it| *it.borrow_mut() = font);
}

pub fn font_fallbacks() -> Vec<FontArc> {
    let mut chain = Vec::new();
    FALLBACK.with(|it| {
        if let Some(f) = it.borrow().clone() {
            chain.push(f);
        }
    });
    CJK_FALLBACK.with(|it| {
        if let Some(f) = it.borrow().clone() {
            chain.push(f);
        }
    });
    chain
}

pub fn cjk_fallbacks() -> Vec<FontArc> {
    CJK_FALLBACK.with(|it| it.borrow().clone().into_iter().collect())
}

fn sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
fn load_font_with_cksum(data: Vec<u8>) -> Result<(FontArc, String)> {
    let cksum = sha256(&data);
    Ok((FontArc::try_from_vec(data)?, cksum))
}

fn set_bold_font((font, cksum): (FontArc, String)) {
    let fallbacks = font_fallbacks();
    BOLD_FONT.with(move |it| *it.borrow_mut() = Some(TextPainter::new(font, fallbacks)));
    BOLD_FONT_CKSUM.with(move |it| *it.borrow_mut() = Some(cksum));
}

impl SharedState {
    pub async fn new(fallback: FontArc) -> Result<Self> {
        FALLBACK.with(|it| *it.borrow_mut() = Some(fallback));
        let path: PathBuf = dir::bold_font_path()?.into();
        let mut font = None;
        if path.exists() {
            font = std::fs::read(&path).ok().and_then(|it| load_font_with_cksum(it).ok());
        }
        let loaded = match font {
            Some(it) => it,
            None => load_font_with_cksum(load_file("bold.ttf").await?)?,
        };
        set_bold_font(loaded);
        Ok(Self {
            t: 0.,
            rt: 0.,
            fader: Fader::new(),
            charts_local: Vec::new(),

            icons: Resource::load_icons().await?,
        })
    }

    pub fn update(&mut self, tm: &mut TimeManager) {
        self.t = tm.now() as _;
        self.rt = tm.real_time() as _;
    }

    pub fn render_fader<R>(&mut self, ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.fader.render(ui, self.t, f)
    }

    pub fn reload_local_charts(&mut self) {
        let mut all = load_local();
        all.extend(load_special_charts());
        self.charts_local = all;
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub enum NextPage {
    #[default]
    None,
    Overlay(Box<dyn Page>),
    Pop,
}

pub trait Page {
    fn label(&self) -> Cow<'static, str>;

    fn custom_title(&self) -> bool {
        false
    }

    fn can_play_bgm(&self) -> bool {
        true
    }
    fn on_result(&mut self, _result: Box<dyn Any>, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }
    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()>;
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool>;
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()>;
    fn render_top(&mut self, _ui: &mut Ui, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }
    fn pause(&mut self) -> Result<()> {
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        Ok(())
    }
    fn next_page(&mut self) -> NextPage {
        NextPage::None
    }
    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        NextScene::None
    }
    fn exit(&mut self) -> Result<()> {
        Ok(())
    }
    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        false
    }
}
