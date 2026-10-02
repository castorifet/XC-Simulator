xcsim_core_l10n::tl_file!("import" itl);

#[path = "scene_app/chart_order_scene.rs"] mod chart_order;
pub use chart_order::ChartOrder;

#[path = "scene_app/chapter_scene.rs"] mod chapter;
pub use chapter::ChapterScene;

#[path = "scene_app/bm_chapter_scene.rs"] mod bm_chapter;
pub use bm_chapter::ChaptersScene;

#[path = "scene_app/main.rs"] mod main;
pub use main::{MainScene, BGM_VOLUME_UPDATED, MP_PANEL};

#[path = "scene_app/song_scene.rs"] mod song;
pub use song::{compress_folder, Downloading, SongScene, RECORD_ID};
#[cfg(feature = "video")]
#[path = "scene_app/unlock_scene.rs"] mod unlock;
#[cfg(feature = "video")]
pub use unlock::UnlockScene;

#[path = "scene_app/profile_scene.rs"] mod profile;
pub use profile::ProfileScene;

use crate::{
    client::{Client, UserManager},
    data::LocalChart,
    dir, get_data, get_data_mut,
    page::Fader,
    save_data,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use once_cell::sync::{Lazy, OnceCell};
use xcsim_core::{
    config::Mods,
    core::PGR_FONT,
    ext::{semi_white, unzip_into, RectExt, SafeTexture},
    fs::{self, FileSystem},
    info::{ChartFormat, ChartInfo},
    parse::has_new_speed_events,
    scene::{show_error, FullLoadingView, GameScene},
    task::Task,
    ui::{Dialog, RectButton, Scroll, Scroller, Ui},
};
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tracing::info;
use uuid::Uuid;

thread_local! {
    pub static TEX_BACKGROUND: RefCell<Option<SafeTexture>> = const { RefCell::new(None) };
    pub static TEX_ICON_BACK: RefCell<Option<SafeTexture>> = const { RefCell::new(None) };
}

pub static ASSET_CHART_INFO: Lazy<Mutex<Option<ChartInfo>>> = Lazy::new(Mutex::default);
pub static TERMS: OnceCell<Option<(String, String)>> = OnceCell::new();
type LoadTosTask = Task<Result<Option<(String, String)>>>;
pub static LOAD_TOS_TASK: Lazy<Mutex<Option<LoadTosTask>>> = Lazy::new(Mutex::default);
pub static JUST_ACCEPTED_TOS: Lazy<AtomicBool> = Lazy::new(AtomicBool::default);
pub static JUST_LOADED_TOS: Lazy<AtomicBool> = Lazy::new(AtomicBool::default);

#[derive(Clone)]
#[allow(dead_code)]
pub struct AssetsChartFileSystem(pub String, pub String);

#[async_trait]
impl FileSystem for AssetsChartFileSystem {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> {
        if path == ":info" {
            return Ok(serde_yaml::to_string(&ASSET_CHART_INFO.lock().unwrap().clone())?.into_bytes());
        }
        #[cfg(closed)]
        {
            use crate::load_res;
            if path == ":music" {
                return Ok(load_res(&format!("res/song/{}/music", self.0)).await);
            }
            if path == ":illu" {
                return Ok(load_res(&format!("res/song/{}/cover", self.0)).await);
            }
            if path == ":chart" {
                return Ok(load_res(&format!("res/song/{}/{}", self.0, self.1)).await);
            }
        }
        bail!("not found");
    }

    async fn exists(&mut self, _path: &str) -> Result<bool> {
        Ok(false)
    }

    fn list_root(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn clone_box(&self) -> Box<dyn FileSystem> {
        Box::new(self.clone())
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub fn fs_from_path(path: &str) -> Result<Box<dyn FileSystem + Send + Sync + 'static>> {
    if let Some(name) = path.strip_prefix(':') {
        let (name, diff) = name.split_once(':').unwrap();
        Ok(Box::new(AssetsChartFileSystem(name.to_owned(), diff.to_owned())))
    } else if let Some(rest) = path.strip_prefix("@chap/") {
        let on_disk = Path::new(&format!("assets/charts/chap/{rest}")).to_path_buf();
        if on_disk.exists() {
            fs::fs_from_file(&on_disk)
        } else {
            fs::fs_from_assets(format!("charts/chap/{rest}/"))
        }
    } else if let Some(name) = path.strip_prefix("@special/") {
        let on_disk = Path::new(&format!("assets/charts/special/{name}")).to_path_buf();
        if on_disk.exists() {
            fs::fs_from_file(&on_disk)
        } else {
            fs::fs_from_assets(format!("charts/special/{name}/"))
        }
    } else {
        fs::fs_from_file(Path::new(&format!("{}/{path}", dir::charts()?)))
    }
}

pub fn confirm_dialog(title: impl Into<String>, content: impl Into<String>, res: Arc<AtomicBool>) {
    Dialog::plain(title.into(), content.into())
        .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
        .listener(move |_dialog, id| {
            if id == 1 {
                res.store(true, Ordering::SeqCst);
            }
            false
        })
        .show();
}

pub fn check_read_tos_and_policy(_change_just_accepted: bool, _strict: bool) -> bool {

    true
}

pub fn dispatch_tos_task() -> Option<bool> {
    let mut tos_task = LOAD_TOS_TASK.lock().unwrap();
    if let Some(task) = &mut *tos_task {
        if let Some(result) = task.take() {
            match result {
                Ok(res) => {
                    if res.is_some() {
                        info!("terms and policy loaded");
                        get_data_mut().terms_modified = None;
                        let _ = save_data();
                        let _ = TERMS.set(res);
                    }

                }
                Err(e) => {
                    show_error(e.context(ttl!("fetch-tos-policy-failed")));
                    *tos_task = None;
                    return Some(false);
                }
            }
            *tos_task = None;
        }
    }
    drop(tos_task);
    None
}

pub fn load_tos_and_policy(strict: bool, show_loading: bool) {
    if TERMS.get().is_some() {
        return;
    }
    let mut guard = LOAD_TOS_TASK.lock().unwrap();
    if guard.is_none() {
        let modified = get_data().terms_modified.clone();
        let loading = show_loading.then(|| FullLoadingView::begin_text(ttl!("loading_tos_policy")));
        *guard = Some(Task::new(async move {
            let mut modified = modified.as_deref();
            if strict {
                modified = None
            }
            let ret = Client::fetch_terms(modified).await.context("failed to fetch terms");
            drop(loading);
            JUST_LOADED_TOS.store(true, Ordering::Relaxed);
            ret
        }));
    }
}

#[inline]
pub fn confirm_delete(res: Arc<AtomicBool>) {
    confirm_dialog(ttl!("del-confirm"), ttl!("del-confirm-content"), res)
}

pub fn gen_custom_dir() -> Result<(PathBuf, Uuid)> {
    let dir = dir::custom_charts()?;
    let dir = Path::new(&dir);
    let mut id = Uuid::new_v4();
    while dir.join(id.to_string()).exists() {
        id = Uuid::new_v4();
    }
    let dir = dir.join(id.to_string());
    std::fs::create_dir(&dir)?;

    Ok((dir, id))
}

#[derive(Debug, Default)]
pub struct ImportWarnings {
    pub has_new_speed_events: bool,
}
impl ImportWarnings {
    pub fn to_string(&self) -> Option<String> {
        let mut warnings = vec![];
        if self.has_new_speed_events {
            warnings.push(format!("- {}", itl!("warning-new-speed-event")));
        }
        if warnings.is_empty() {
            None
        } else {
            Some(warnings.join("\n"))
        }
    }
}

async fn check_speed(fs: &mut dyn FileSystem, info: &ChartInfo, warnings: &mut ImportWarnings) -> Result<()> {
    let bytes = GameScene::load_chart_bytes(fs, info).await.context("Failed to load chart")?;
    let format = GameScene::infer_chart_format(info, &bytes);
    if format != ChartFormat::Rpe {
        return Ok(());
    }
    let source = String::from_utf8_lossy(&bytes);
    if has_new_speed_events(&source).await? {
        warnings.has_new_speed_events = true;
    }

    Ok(())
}

pub async fn import_chart_to(dir: &Path, local_path: String, file: File) -> Result<(LocalChart, ImportWarnings)> {
    let mut warnings = ImportWarnings::default();
    let dir = xcsim_core::dir::Dir::new(dir)?;
    unzip_into(BufReader::new(file), &dir, true)?;
    let mut fs = fs_from_path(&local_path)?;
    let mut info = fs::load_info(fs.as_mut()).await.with_context(|| itl!("info-fail"))?;
    fs::fix_info(fs.as_mut(), &mut info).await.with_context(|| itl!("invalid-chart"))?;
    if info.use_rpe_170_speed.is_none() {
        check_speed(fs.as_mut(), &info, &mut warnings).await?;
    }
    dir.create("info.yml")?.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
    Ok((
        LocalChart {
            info: info.into(),
            local_path,
            record: None,
            mods: Mods::default(),
            played_unlock: false,
        },
        warnings,
    ))
}

pub async fn import_chart(file: File) -> Result<(LocalChart, ImportWarnings)> {
    let (dir, id) = gen_custom_dir()?;
    match import_chart_to(&dir, format!("custom/{id}"), file).await {
        Err(err) => {
            std::fs::remove_dir_all(dir)?;
            Err(err)
        }
        Ok(val) => Ok(val),
    }
}

pub struct LdbDisplayItem<'a> {
    pub player_id: i32,
    pub rank: u32,
    pub score: String,
    pub alt: Option<String>,
    pub btn: &'a mut RectButton,
}

#[allow(clippy::too_many_arguments)]
pub fn render_ldb<'a>(
    ui: &mut Ui,
    title: &str,
    w: f32,
    rt: f32,
    scroll: &mut Scroll,
    fader: &mut Fader,
    icon_user: &SafeTexture,
    iter: Option<impl Iterator<Item = LdbDisplayItem<'a>>>,
) {
    use macroquad::prelude::*;

    let pad = 0.03;
    let width = w - pad;
    ui.dy(0.01);
    let r = ui.text(title).size(0.9).draw();
    ui.dy(r.h + 0.05);
    let sh = ui.top * 2. - r.h - 0.08;
    let Some(iter) = iter else {
        ui.loading(width / 2., sh / 2., rt, WHITE, ());
        return;
    };
    let off = scroll.y_scroller.offset;
    scroll.size((width, sh));
    scroll.render(ui, |ui| {
        render_release_to_refresh(ui, width / 2., off);
        let s = 0.14;
        let mut h = 0.;
        ui.dx(0.02);
        fader.reset();
        let me = get_data().me.as_ref().map(|it| it.id);
        fader.for_sub(|f| {
            for item in iter {
                f.render(ui, rt, |ui| {
                    if me == Some(item.player_id) {
                        ui.fill_path(&Rect::new(-0.02, 0., width, s).feather(-0.01).rounded(0.02), ui.background());
                    }
                    let r = s / 2. - 0.02;
                    ui.text(format!("#{}", item.rank))
                        .pos((0.18 - r) / 2., s / 2.)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.52)
                        .draw_using(&PGR_FONT);
                    let ct = (0.18, s / 2.);
                    ui.avatar(ct.0, ct.1, r, rt, UserManager::opt_avatar(item.player_id, icon_user));
                    item.btn.set(ui, Rect::new(ct.0 - r, ct.1 - r, r * 2., r * 2.));
                    let mut rt = width - 0.04;
                    if let Some(alt) = item.alt {
                        let r = ui
                            .text(alt)
                            .pos(rt, s / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .size(0.4)
                            .color(semi_white(0.6))
                            .draw();
                        rt -= r.w + 0.01;
                    } else {
                        rt -= 0.01;
                    }
                    let r = ui
                        .text(item.score)
                        .pos(rt, s / 2.)
                        .anchor(1., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .draw_using(&PGR_FONT);
                    rt -= r.w + 0.03;
                    let lt = 0.25;
                    if let Some((name, color)) = UserManager::name_and_color(item.player_id) {
                        ui.text(name)
                            .pos(lt, s / 2.)
                            .anchor(0., 0.5)
                            .no_baseline()
                            .max_width(rt - lt - 0.01)
                            .size(0.5)
                            .color(color)
                            .draw();
                    }
                });
                ui.dy(s);
                h += s;
            }
        });
        (width, h)
    });
}

pub fn render_release_to_refresh(ui: &mut Ui, cx: f32, off: f32) {
    let p = (-off / Scroller::EXTEND).clamp(0., 1.);
    ui.text(ttl!("release-to-refresh"))
        .pos(cx, -0.2 + p * 0.07)
        .anchor(0.5, 0.)
        .size(0.8)
        .color(semi_white(p * 0.8))
        .draw();
}

#[cfg(test)]
mod tests {

}
