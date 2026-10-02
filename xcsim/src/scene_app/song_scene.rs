xcsim_core_l10n::tl_file!("song");

#[cfg(feature = "video")]
use super::UnlockScene;
use super::{
    confirm_delete, confirm_dialog, fs_from_path, gen_custom_dir, import_chart_to, render_ldb, LdbDisplayItem, ProfileScene, ASSET_CHART_INFO,
};
use crate::{
    charts_view::NEED_UPDATE,
    client::{
        basic_client_builder, recv_raw, Chart, ChartRef, Client, Collection, CollectionPatch, Permissions, Ptr, Record, UserManager, CLIENT_TOKEN,
    },
    data::{BriefChartInfo, LocalChart},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{local_illustration, thumbnail_path, ChartItem, ChartType, Fader, Illustration, SFader, FAV_UPDATED},
    popup::Popup,
    rate::RateDialog,
    save_data,
    tags::TagsDialog,
};
use ::rand::{thread_rng, Rng};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use core::f32;
use futures_util::StreamExt;
use inputbox::{InputBox, InputMode};
use macroquad::prelude::*;
use xc_sim_mp_common::{ClientCommand, CompactPos, JudgeEvent, TouchFrame};
use xcsim_core::{
    config::Mods,
    core::Tweenable,
    ext::{
        open_url, poll_future, rect_shadow, semi_black, semi_white, unzip_into, JoinToString, LocalTask, RectExt, SafeTexture, ScaleType,
        BLACK_TEXTURE,
    },
    fs::{self},
    info::ChartInfo,
    judge::{icon_index, Judge},
    scene::{
        request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input, BasicPlayer, GameMode, LoadingScene,
        LocalSceneTask, NextScene, RecordUpdateState, SaveFn, Scene, SimpleRecord, UpdateFn, UploadFn,
    },
    task::Task,
    time::TimeManager,
    ui::{button_hit, handle_chart_info_files, render_chart_info_tab, ChartInfoEdit, ChartInfoPage, EDIT_TAB_COUNT, EDIT_TAB_LABELS, DRectButton, Dialog, LongTouchState, RectButton, Scroll, Ui, UI_AUDIO},
};
use reqwest::Method;
use sasa::{AudioClip, Frame, Music, MusicParams};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    borrow::Cow,
    collections::{hash_map, HashMap, VecDeque},
    fs::File,
    io::{Cursor, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc, Mutex, Weak,
    },
    thread_local,
};
use tap::Tap;
use tokio::net::TcpStream;
use tracing::{error, warn};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

type LocalTuple = (String, ChartInfo, AudioClip, Illustration);

static CONFIRM_CKSUM: AtomicBool = AtomicBool::new(false);
static UPLOAD_NOT_SAVED: AtomicBool = AtomicBool::new(false);
static CONFIRM_OVERWRITE: AtomicBool = AtomicBool::new(false);
static CONFIRM_UPLOAD: AtomicBool = AtomicBool::new(false);
pub static RECORD_ID: AtomicI32 = AtomicI32::new(-1);

fn fade_in_time() -> Option<f32> {
    if get_data().prefer_reduced_motion {
        None
    } else {
        Some(0.3)
    }
}

fn edit_transit() -> Option<f32> {
    if get_data().prefer_reduced_motion {
        None
    } else {
        Some(0.32)
    }
}

fn create_music(clip: AudioClip) -> Result<Music> {
    let mut music = UI_AUDIO.with(|it| {
        it.borrow_mut().create_music(
            clip,
            MusicParams {
                amplifier: 0.7,
                loop_mix_time: 0.,
                ..Default::default()
            },
        )
    })?;
    music.play()?;
    Ok(music)
}

fn with_effects((mut frames, sample_rate): (Vec<Frame>, u32), range: Option<(f32, f32)>) -> Result<AudioClip> {
    if let Some((begin, end)) = range {
        frames.drain(((end * sample_rate as f32) as usize).min(frames.len())..);
        frames.drain(..((begin * sample_rate as f32) as usize));
    }
    let len = (0.8 * sample_rate as f64) as usize;
    let len = len.min(frames.len() / 2);
    for (i, frame) in frames[..len].iter_mut().enumerate() {
        let s = i as f32 / len as f32;
        frame.0 *= s;
        frame.1 *= s;
    }
    let st = frames.len() - len;
    for (i, frame) in frames[st..].iter_mut().rev().enumerate() {
        let s = i as f32 / len as f32;
        frame.0 *= s;
        frame.1 *= s;
    }
    Ok(AudioClip::from_raw(frames, sample_rate))
}

async fn load_local_tuple(local_path: &str, def_illu: SafeTexture, info: ChartInfo) -> Result<LocalTuple> {
    let dir = xcsim_core::dir::Dir::new(format!("{}/{local_path}", dir::charts()?))?;
    let bytes = dir.read(&info.music)?;
    let (frames, sample_rate) = AudioClip::decode(bytes)?;
    let length = frames.len() as f32 / sample_rate as f32;
    if info.preview_end.unwrap_or(info.preview_start + 1.) > length {
        tl!(bail "edit-preview-invalid");
    }
    let preview = with_effects((frames, sample_rate), Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))))?;
    let illu = local_illustration(local_path.to_owned(), def_illu, true);
    illu.notify.notify_one();

    Ok((local_path.to_owned(), info, preview, illu))
}

pub struct Downloading {
    info: BriefChartInfo,
    local_path: Option<String>,
    loading_last: f32,
    cancel_download_btn: DRectButton,
    status: Arc<Mutex<Cow<'static, str>>>,
    prog: Arc<Mutex<Option<f32>>>,
    atomicity: Arc<Mutex<()>>,
    task: Task<Result<(LocalChart, LocalTuple)>>,
}

impl Downloading {
    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.cancel_download_btn.touch(touch, t)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), semi_black(0.6));
        ui.loading(0., -0.06, t, WHITE, (*self.prog.lock().unwrap(), &mut self.loading_last));
        ui.text(self.status.lock().unwrap().clone())
            .pos(0., 0.02)
            .anchor(0.5, 0.)
            .size(0.6)
            .draw();
        let size = 0.7;
        let r = ui.text(tl!("dl-cancel")).pos(0., 0.12).anchor(0.5, 0.).size(size).measure().feather(0.02);
        self.cancel_download_btn.render_text(ui, r, t, tl!("dl-cancel"), 0.6, true);
    }

    pub fn check(&mut self) -> Result<Option<Option<LocalTuple>>> {
        if let Some(res) = self.task.take() {
            match res {
                Err(err) => {
                    let path = format!("{}/{}", dir::downloaded_charts()?, self.info.id.unwrap());
                    let path = Path::new(&path);
                    if path.exists() {
                        std::fs::remove_dir_all(path)?;
                    }
                    show_error(err.context(tl!("dl-failed")));
                    Ok(Some(None))
                }
                Ok((chart, tuple)) => {
                    self.info = chart.info.clone();
                    if let Some(local_path) = &self.local_path {

                        SongScene::global_update_chart_info(local_path, self.info.clone())?;
                    } else {
                        NEED_UPDATE.store(true, Ordering::Relaxed);
                        self.local_path = Some(chart.local_path.clone());
                        get_data_mut().charts.push(chart);
                    }
                    save_data()?;
                    show_message(tl!("dl-success")).ok();
                    Ok(Some(Some(tuple)))
                }
            }
        } else {
            Ok(None)
        }
    }
}

enum SideContent {
    Edit,
    Leaderboard,
    Info,
    Mods,
}

impl SideContent {
    fn width(&self) -> f32 {
        match self {
            Self::Edit => 2.0,
            Self::Leaderboard => 0.94,
            Self::Info => 0.75,
            Self::Mods => 0.62,
        }
    }
}

const MOD_FLAGS: [Mods; 4] = [Mods::AUTOPLAY, Mods::FLIP_X, Mods::FADE_IN, Mods::FADE_OUT];
const MOD_TITLES: [&str; 4] = ["Autoplay", "Flip X", "Fade In", "Fade Out"];
const MOD_SUBS: [&str; 4] = [
    "Auto-play the chart.",
    "Mirror the chart horizontally.",
    "Notes fade in from invisible.",
    "Notes fade out before hitting.",
];

#[allow(clippy::too_many_arguments)]
fn kv_row(ui: &mut Ui, lx: f32, vx: f32, y: f32, label: &str, value: &str, lc: Color, vc: Color, max_w: f32) {
    ui.text(label).pos(lx, y).anchor(0., 0.5).no_baseline().size(0.36).color(lc).draw();
    ui.text(value).pos(vx, y).anchor(0., 0.5).no_baseline().size(0.4).max_width(max_w).color(vc).draw();
}

#[derive(Deserialize)]
struct StableR {
    status: i8,
}

#[derive(Deserialize)]
struct LdbItem {
    #[serde(flatten)]
    pub inner: Record,
    pub rank: u32,
    #[serde(skip, default)]
    pub btn: RectButton,
}

pub struct SongScene {
    illu: Illustration,

    first_in: bool,

    back_btn: RectButton,
    play_btn: DRectButton,

    icons: Arc<Icons>,

    next_scene: Option<NextScene>,

    preview: Option<Music>,
    preview_task: Option<Task<Result<AudioClip>>>,

    load_task: Option<Task<Result<Option<Arc<Chart>>>>>,
    entity: Option<Chart>,
    info: BriefChartInfo,
    local_path: Option<String>,

    downloading: Option<Downloading>,
    loading_last: f32,

    rank_icons: [SafeTexture; 8],
    record: Option<SimpleRecord>,

    fetch_best_task: Option<Task<Result<SimpleRecord>>>,

    menu: Popup,
    menu_btn: RectButton,
    need_show_menu: bool,
    should_delete: Arc<AtomicBool>,
    menu_options: Vec<&'static str>,

    info_page: Option<ChartInfoPage>,
    edit_btn: RectButton,
    btn_edit_save: DRectButton,
    btn_edit_cancel: DRectButton,
    btn_edit_tags_page: DRectButton,
    btn_edit_upload: DRectButton,

    mods: Mods,

    mod_btn: DRectButton,
    mod_toggle_btns: [RectButton; 4],
    mod_exit_btn: RectButton,

    chart_music: String,
    chart_illu: String,

    side_content: SideContent,
    side_enter_time: f32,

    save_task: Option<Task<Result<LocalTuple>>>,
    upload_task: Option<Task<Result<BriefChartInfo>>>,

    ldb: Option<(Option<u32>, Vec<LdbItem>)>,
    ldb_task: Option<Task<Result<Vec<LdbItem>>>>,
    ldb_btn: RectButton,
    ldb_scroll: Scroll,
    ldb_fader: Fader,
    ldb_type_btn: DRectButton,
    ldb_std: bool,

    info_btn: RectButton,
    info_scroll: Scroll,

    si_scroll: Scroll,
    desc_scroll: Scroll,

    fav_btn: RectButton,
    fav_long_touch: LongTouchState,
    fav_menu: Popup,
    fav_menu_options: Vec<Uuid>,
    need_show_fav_menu: bool,

    review_task: Option<Task<Result<String>>>,
    chart_should_delete: Arc<AtomicBool>,
    should_review_approve: Arc<AtomicBool>,

    edit_tags_task: Option<Task<Result<()>>>,
    tags: TagsDialog,

    rate_dialog: RateDialog,
    rate_task: Option<Task<Result<()>>>,

    should_update: Arc<AtomicBool>,

    my_rating_task: Option<Task<Result<i16>>>,
    my_rate_score: Option<i16>,

    stabilize_task: Option<Task<Result<()>>>,
    should_stabilize: Arc<AtomicBool>,
    should_stabilize_approve: Arc<AtomicBool>,
    should_stabilize_approve_ranked: Arc<AtomicBool>,

    scene_task: LocalTask<Result<NextScene>>,

    uploader_btn: RectButton,

    sf: SFader,
    fade_start: f32,

    background: Arc<Mutex<Option<SafeTexture>>>,
    tr_start: f32,

    open_web_btn: DRectButton,

    overwrite_from: Option<String>,
    overwrite_task: Option<Task<Result<LocalTuple>>>,

    update_cksum_passed: Option<bool>,
    update_cksum_task: Option<Task<Result<bool>>>,
    chart_type: ChartType,

    toggle_fav_task: Option<Task<Result<(Collection, bool)>>>,

    confirm_cancel_edit: Arc<AtomicBool>,
}

impl SongScene {
    pub fn new(mut chart: ChartItem, local_path: Option<String>, icons: Arc<Icons>, rank_icons: [SafeTexture; 8], mods: Mods) -> Self {
        if let Some(path) = &local_path {
            if let Some(id) = path.strip_prefix("download/") {
                chart.info.id = Some(id.parse().unwrap());
            }
        }
        let illu = if let Some(path) = &chart.local_path {
            let illu = local_illustration(path.clone(), chart.illu.texture.1.clone(), true);
            illu.notify.notify_one();
            illu
        } else if let Some(id) = chart.info.id {
            Illustration {
                texture: chart.illu.texture.clone(),
                notify: Arc::default(),
                task: Some(Task::new({
                    async move {
                        let chart = Ptr::<Chart>::new(id).load().await?;
                        let image = chart.illustration.load_image().await?;
                        Ok((image, None))
                    }
                })),
                loaded: Arc::default(),
                load_time: f32::NAN,
            }
        } else {
            chart.illu
        };
        let record = get_data()
            .charts
            .iter()
            .find(|it| Some(&it.local_path) == local_path.as_ref())
            .and_then(|it| it.record.clone())
            .or_else(|| local_path.as_ref().and_then(|path| get_data().local_records.get(path).cloned().flatten()));
        let fetch_best_task = if get_data().me.is_some() {
            chart.info.id.map(|id| Task::new(Client::best_record(id)))
        } else {
            None
        };
        let id = chart.info.id;
        let offline_mode = get_data().config.offline_mode;
        let _icon_star = icons.star.clone();
        Self {
            illu,

            first_in: true,

            back_btn: RectButton::new(),
            play_btn: DRectButton::new(),

            icons,

            next_scene: None,

            preview: None,
            preview_task: Some(Task::new({
                let local_path = local_path.clone();
                async move {
                    if let Some(path) = local_path {
                        let mut fs = fs_from_path(&path)?;
                        let info = fs::load_info(fs.as_mut()).await?;
                        with_effects(
                            AudioClip::decode(fs.load_file(&info.music).await?)?,
                            Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))),
                        )
                    } else {
                        let chart = Ptr::<Chart>::new(id.unwrap()).fetch().await?;
                        with_effects(AudioClip::decode(chart.preview.fetch().await?.to_vec())?, None)
                    }
                }
            })),

            load_task: if offline_mode {
                None
            } else {
                id.map(|it| Task::new(async move { Ptr::new(it).fetch_opt().await }))
            },
            entity: None,
            info: chart.info,
            local_path,

            downloading: None,
            loading_last: 0.,

            rank_icons,
            record,

            fetch_best_task,

            menu: Popup::new(),
            menu_btn: RectButton::new(),
            need_show_menu: false,
            should_delete: Arc::new(AtomicBool::default()),
            menu_options: Vec::new(),

            info_page: None,
            edit_btn: RectButton::new(),
            btn_edit_save: DRectButton::new(),
            btn_edit_cancel: DRectButton::new(),
            btn_edit_tags_page: DRectButton::new(),
            btn_edit_upload: DRectButton::new(),

            mods,

            mod_btn: DRectButton::new(),
            mod_toggle_btns: Default::default(),
            mod_exit_btn: RectButton::new(),

            chart_music: String::new(),
            chart_illu: String::new(),

            side_content: SideContent::Edit,
            side_enter_time: f32::INFINITY,

            save_task: None,
            upload_task: None,

            ldb: None,
            ldb_task: None,
            ldb_btn: RectButton::new(),
            ldb_scroll: Scroll::new(),
            ldb_fader: Fader::new().with_distance(0.12),
            ldb_type_btn: DRectButton::new(),
            ldb_std: false,

            info_btn: RectButton::new(),
            info_scroll: Scroll::new(),

            si_scroll: Scroll::new(),
            desc_scroll: Scroll::new(),

            fav_btn: RectButton::new(),
            fav_long_touch: LongTouchState::default(),
            fav_menu: Popup::new().tap_mut(|it| it.set_auto_dismiss(false)),
            fav_menu_options: Vec::new(),
            need_show_fav_menu: false,

            review_task: None,
            chart_should_delete: Arc::default(),
            should_review_approve: Arc::default(),

            edit_tags_task: None,
            tags: TagsDialog::new(false),

            rate_dialog: RateDialog::new(false),
            rate_task: None,

            should_update: Arc::default(),

            my_rating_task: if offline_mode {
                None
            } else {
                id.map(|id| {
                    Task::new(async move {
                        #[derive(Deserialize)]
                        struct Resp {
                            score: i16,
                        }
                        let resp: Resp = recv_raw(Client::get(format!("/chart/{id}/rate"))).await?.json().await?;
                        Ok(resp.score)
                    })
                })
            },
            my_rate_score: None,

            stabilize_task: None,
            should_stabilize: Arc::default(),
            should_stabilize_approve: Arc::default(),
            should_stabilize_approve_ranked: Arc::default(),

            scene_task: None,

            uploader_btn: RectButton::new(),

            sf: SFader::new(),
            fade_start: 0.,

            tr_start: f32::NAN,
            background: Arc::default(),

            open_web_btn: DRectButton::new(),

            overwrite_from: None,
            overwrite_task: None,

            update_cksum_passed: None,
            update_cksum_task: None,
            chart_type: chart.chart_type,

            toggle_fav_task: None,

            confirm_cancel_edit: Arc::default(),
        }
    }

    fn start_download(&mut self) -> Result<()> {
        let chart = self.info.clone();
        let Some(entity) = self.entity.clone() else {
            show_message(tl!("still-loading")).error();
            return Ok(());
        };
        self.loading_last = 0.;
        self.downloading = Some(Self::global_start_download(chart, entity, self.local_path.clone())?);
        Ok(())
    }

    pub fn global_start_download(chart: BriefChartInfo, entity: Chart, local_path: Option<String>) -> Result<Downloading> {
        let progress = Arc::new(Mutex::new(None));
        let prog_wk = Arc::downgrade(&progress);
        let status = Arc::new(Mutex::new(tl!("dl-status-fetch")));
        let status_shared = Arc::clone(&status);
        let atomicity = Arc::new(Mutex::new(()));
        Ok(Downloading {
            info: chart.clone(),
            local_path,
            loading_last: 0.,
            cancel_download_btn: DRectButton::new(),
            prog: progress,
            status: status_shared,
            atomicity: atomicity.clone(),
            task: Task::new({
                let path = format!("{}/{}", dir::downloaded_charts()?, Uuid::new_v4());
                async move {
                    let path = std::path::Path::new(&path);
                    tokio::fs::create_dir(path).await?;
                    let dir = xcsim_core::dir::Dir::new(path)?;

                    let chart = chart;
                    async fn download(mut file: impl Write, url: &str, prog_wk: &Weak<Mutex<Option<f32>>>) -> Result<()> {
                        let Some(prog) = prog_wk.upgrade() else { return Ok(()) };
                        *prog.lock().unwrap() = None;
                        let req = basic_client_builder().build().unwrap().get(url);
                        let req = if let Some(token) = CLIENT_TOKEN.load().as_ref() {
                            req.header("Authorization", format!("Bearer {token}"))
                        } else {
                            req
                        };
                        let res = req.send().await.with_context(|| tl!("request-failed"))?.error_for_status()?;
                        let size = res.content_length();
                        let mut stream = res.bytes_stream();
                        let mut count = 0;
                        while let Some(chunk) = stream.next().await {
                            let chunk = chunk?;
                            file.write_all(&chunk)?;
                            count += chunk.len() as u64;
                            if let Some(size) = size {
                                *prog.lock().unwrap() = Some(count.min(size) as f32 / size as f32);
                            }
                            if prog_wk.strong_count() == 1 {

                                break;
                            }
                        }
                        Ok(())
                    }

                    *status.lock().unwrap() = tl!("dl-status-chart");
                    let mut bytes = Vec::new();
                    download(Cursor::new(&mut bytes), &entity.file.url, &prog_wk).await?;
                    *status.lock().unwrap() = tl!("dl-status-extract");
                    if prog_wk.strong_count() != 0 {
                        unzip_into(Cursor::new(bytes), &dir, false)?;
                    }
                    *status.lock().unwrap() = tl!("dl-status-saving");
                    if let Some(prog) = prog_wk.upgrade() {
                        *prog.lock().unwrap() = None;
                    }
                    let mut info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
                    info.id = Some(entity.id);
                    info.created = Some(entity.created);
                    info.updated = Some(entity.updated);
                    info.chart_updated = Some(entity.chart_updated);
                    info.uploader = Some(entity.uploader.id);
                    serde_yaml::to_writer(dir.create("info.yml")?, &info)?;

                    if prog_wk.strong_count() == 0 {

                        drop(dir);
                        tokio::fs::remove_dir_all(&path).await?;
                    }

                    let local_path = format!("download/{}", chart.id.unwrap());
                    let to_path = format!("{}/{local_path}", dir::charts()?);
                    let to_path = Path::new(&to_path);
                    {
                        let _guard = atomicity.lock().unwrap();
                        if to_path.exists() {
                            if to_path.is_file() {
                                std::fs::remove_file(to_path)?;
                            } else {
                                std::fs::remove_dir_all(to_path)?;
                            }
                        }
                        std::fs::rename(path, to_path)?;
                    }

                    let tuple = load_local_tuple(&local_path, BLACK_TEXTURE.clone(), info).await?;

                    Ok((
                        LocalChart {
                            info: entity.to_info(),
                            local_path,
                            record: None,
                            mods: Mods::default(),
                            played_unlock: false,
                        },
                        tuple,
                    ))
                }
            }),
        })
    }

    fn load_ldb(&mut self) {
        if get_data().config.offline_mode {
            return;
        }
        let Some(id) = self.info.id else { return };
        self.ldb = None;
        let std = self.ldb_std;
        self.ldb_task = Some(Task::new(async move {
            Ok(recv_raw(Client::get(format!("/record/list15/{id}")).query(&[("std", std)]))
                .await?
                .json()
                .await?)
        }));
    }

    fn update_record(&mut self, new_rec: SimpleRecord) -> Result<()> {
        let rec = get_data_mut()
            .charts
            .iter_mut()
            .find(|it| Some(&it.local_path) == self.local_path.as_ref())
            .map(|it| &mut it.record)
            .or_else(|| {
                self.local_path
                    .clone()
                    .map(|path| get_data_mut().local_records.entry(path).or_insert(None))
            });
        let Some(rec) = rec else {
            if let Some(rec) = &mut self.record {
                rec.update(&new_rec);
            } else {
                self.record = Some(new_rec);
            }
            return Ok(());
        };
        if let Some(rec) = rec {
            if rec.update(&new_rec) {
                save_data()?;
            }
        } else {
            *rec = Some(new_rec);
            save_data()?;
        }
        self.record = rec.clone();
        Ok(())
    }

    fn update_menu(&mut self) {
        self.menu_options.clear();

        if self
            .local_path
            .as_ref()
            .is_some_and(|it| !it.starts_with(':') && !it.starts_with("@chap/") && !it.starts_with("@special/"))
        {
            self.menu_options.push("delete");
        }
        if self.info.id.is_some() {
            self.menu_options.push("rate");
        }
        if let Some(local_path) = &self.local_path {
            self.menu_options.push("exercise");
            self.menu_options.push("offset");
            if get_data()
                .charts
                .iter()
                .find(|it| it.local_path == *local_path)
                .is_some_and(|it| it.played_unlock)
            {
                self.menu_options.push("unlock");
            }
        }
        let perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default();
        let is_uploader = get_data()
            .me
            .as_ref()
            .is_some_and(|it| Some(it.id) == self.info.uploader.as_ref().map(|it| it.id));
        if self.info.id.is_some() && (perms.contains(Permissions::REVIEW) || perms.contains(Permissions::REVIEW_PECJAM)) {
            if self.entity.as_ref().is_some_and(|it| !it.reviewed && !it.stable_request) {
                self.menu_options.push("review-approve");
                self.menu_options.push("review-deny");
            }
            self.menu_options.push("review-edit-tags");
        }
        if self.info.id.is_some() && is_uploader && self.entity.as_ref().is_some_and(|it| !it.stable && !it.stable_request) {
            self.menu_options.push("stabilize");
        }
        if self.info.id.is_some() && self.entity.as_ref().is_some_and(|it| it.stable_request) && perms.contains(Permissions::STABILIZE_CHART) {
            self.menu_options.push("stabilize-approve");
            self.menu_options.push("stabilize-approve-ranked");
            self.menu_options.push("stabilize-comment");
            self.menu_options.push("stabilize-deny");
        }
        if self.info.id.is_some()
            && self.entity.as_ref().is_some_and(|it| {
                if it.stable {
                    perms.contains(Permissions::DELETE_STABLE)
                } else {
                    is_uploader || perms.contains(Permissions::DELETE_UNSTABLE)
                }
            })
        {
            self.menu_options.push("review-del");
        }
        self.menu.set_options(self.menu_options.iter().map(|it| tl!(*it).into_owned()).collect());
    }

    fn launch(&mut self, mode: GameMode, force_unlock: bool) -> Result<()> {
        let local_path = self.local_path.as_ref().unwrap();
        let is_unlock = force_unlock
            || (mode == GameMode::Normal
                && get_data()
                    .charts
                    .iter()
                    .find(|it| it.local_path == *local_path)
                    .is_some_and(|it| it.info.has_unlock && !it.played_unlock));

        self.scene_task =
            Self::global_launch(self.info.id, local_path, self.mods, mode, None, Some(self.background.clone()), self.record.clone(), is_unlock)?;

        Ok(())
    }

    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[allow(clippy::too_many_arguments)]
    pub fn global_launch(
        id: Option<i32>,
        local_path: &str,
        mods: Mods,
        mode: GameMode,
        client: Option<Arc<xc_sim_mp_client::Client>>,
        background_output: Option<Arc<Mutex<Option<SafeTexture>>>>,
        record: Option<SimpleRecord>,
        is_unlock: bool,
    ) -> Result<LocalSceneTask> {
        let mut fs = fs_from_path(local_path)?;
        let can_rated = id.is_some() || local_path.starts_with(':');
        #[cfg(feature = "video")]
        let local_path = local_path.to_owned();
        #[cfg(closed)]
        let rated = {
            let config = &get_data().config;
            !config.offline_mode && can_rated && !mods.contains(Mods::AUTOPLAY) && config.speed >= 1.0 - 1e-3
        };
        #[cfg(not(closed))]
        let rated = false;
        if !rated && can_rated && mode == GameMode::Normal {
            show_message(tl!("warn-unrated")).warn();
        }
        let update_fn = client.and_then(|mut client| {
            let live = client.blocking_state().unwrap().live;
            let token = get_data().tokens.as_ref().map(|it| it.0.clone()).unwrap();
            let addr = get_data().config.mp_address.clone();
            let mut reconnect_task: Option<Task<Result<xc_sim_mp_client::Client>>> = None;
            let update_fn: Option<UpdateFn> = if live {
                Some(Box::new({
                    let mut touch_ids: HashMap<u64, i8> = HashMap::new();
                    let mut touch_last_update: HashMap<i8, f32> = HashMap::new();
                    let mut touches: VecDeque<TouchFrame> = VecDeque::new();
                    let mut judges: VecDeque<JudgeEvent> = VecDeque::new();
                    let mut last_send_touch_time: f32 = 0.;
                    move |t, res, judge| {
                        if client.ping_fail_count() >= 1 && reconnect_task.is_none() {
                            warn!("lost connection, auto re-connect");
                            let token = token.clone();
                            let addr = addr.clone();
                            reconnect_task = Some(Task::new(async move {
                                let client = xc_sim_mp_client::Client::new(TcpStream::connect(addr).await?).await?;
                                client.authenticate(token).await?;
                                Ok(client)
                            }));
                        }
                        if let Some(task) = &mut reconnect_task {
                            if let Some(res) = task.take() {
                                match res {
                                    Err(err) => {
                                        warn!(?err, "failed to reconnect");
                                    }
                                    Ok(new) => {
                                        warn!("reconnected!");
                                        client = new.into();
                                    }
                                }
                                reconnect_task = None;
                            }
                        }
                        let points: Vec<_> = Judge::get_touches()
                            .into_iter()
                            .filter_map(|it| {
                                if matches!(it.phase, TouchPhase::Stationary) {
                                    return None;
                                }
                                let len = touch_ids.len();
                                let mut id = match touch_ids.entry(it.id) {
                                    hash_map::Entry::Occupied(val) => *val.get(),
                                    hash_map::Entry::Vacant(place) => *place.insert(len.try_into().ok()?),
                                };
                                if matches!(it.phase, TouchPhase::Moved) && touch_last_update.get(&id).is_some_and(|it| *it + 1. / 20. >= t) {
                                    return None;
                                }
                                touch_last_update.insert(id, t);
                                if matches!(it.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                                    touch_ids.remove(&it.id);
                                    id = !id;
                                }
                                Some((id, CompactPos::new(it.position.x, it.position.y * res.aspect_ratio)))
                            })
                            .collect();
                        if !points.is_empty() {
                            touches.push_back(TouchFrame { time: t, points });
                        }
                        if last_send_touch_time + 1. < t || touches.len() > 20 {
                            if touches.is_empty() {
                                touches.push_back(TouchFrame { time: t, points: Vec::new() });
                            }
                            let frames = Arc::new(touches.drain(..).collect());
                            client.blocking_send(ClientCommand::Touches { frames }).unwrap();
                            last_send_touch_time = t;
                        }
                        judges.extend(judge.judgements.borrow_mut().drain(..).map(|it| JudgeEvent {
                            time: it.0,
                            line_id: it.1,
                            note_id: it.2,
                            judgement: {
                                use xc_sim_mp_common::Judgement::*;
                                use xcsim_core::judge::Judgement as OJ;
                                match it.3 {
                                    Ok(OJ::ArcPerfect) => ArcPerfect,
                                    Ok(OJ::Perfect) => Perfect,
                                    Ok(OJ::Good) => Good,
                                    Ok(OJ::Bad) => Bad,
                                    Ok(OJ::Miss) => Miss,
                                    Err(true) => HoldPerfect,
                                    Err(false) => HoldGood,
                                }
                            },
                        }));
                        if judges.len() > 10 || judges.front().is_some_and(|it| it.time + 0.6 < t) {
                            let judges = Arc::new(judges.drain(..).collect());
                            client.blocking_send(ClientCommand::Judges { judges }).unwrap();
                        }
                    }
                }))
            } else {
                None
            };
            update_fn
        });

        let save_fn: Option<SaveFn> = Some(Box::new({
            let local_path = local_path.to_string();
            move |new_rec| -> Result<()> {
                let rec = get_data_mut()
                    .charts
                    .iter_mut()
                    .find(|it| it.local_path == local_path)
                    .map(|it| &mut it.record)
                    .or_else(|| Some(get_data_mut().local_records.entry(local_path.clone()).or_insert(None)))
                    .unwrap();
                if let Some(rec) = rec {
                    if rec.update(&new_rec) {
                        save_data()?;
                    }
                } else {
                    *rec = Some(new_rec);
                    save_data()?;
                }
                Ok(())
            }
        }));

        Ok(Some(Box::pin(async move {
            let mut info = fs::load_info(fs.as_mut()).await?;
            info.id = id;
            let mut config = get_data().config.clone();
            config.player_name = get_data()
                .me
                .as_ref()
                .map(|it| it.name.clone())
                .unwrap_or_else(|| tl!("guest").into_owned());
            config.res_pack_path = {
                let id = get_data().respack_id;
                if id == 0 {
                    None
                } else {
                    Some(format!("{}/{}", dir::respacks()?, get_data().respacks[id - 1]))
                }
            };
            let chart_updated = info.chart_updated;
            config.mods = mods;
            let preload = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
            if let Some(output) = background_output {
                *output.lock().unwrap() = Some(preload.1.clone());
            }
            let player = get_data().me.as_ref().map(|it| BasicPlayer {
                avatar: UserManager::get_avatar(it.id).flatten(),
                id: it.id,
                rks: it.rks,
                historic_best: record.map_or(0, |it| it.score as u32),
            });
            let upload_fn: Option<UploadFn> = Some(Arc::new(move |data: Vec<u8>| {
                Task::new(async move {
                    #[derive(Serialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Req {
                        chart: i32,
                        token: String,
                        chart_updated: Option<DateTime<Utc>>,
                    }
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Resp {
                        id: i32,
                        exp_delta: f64,
                        new_best: bool,
                        improvement: u32,
                        new_rks: f32,
                    }
                    let resp: Resp = recv_raw(Client::post(
                        "/play/upload",
                        &Req {
                            chart: id.unwrap(),
                            token: STANDARD.encode(data),
                            chart_updated,
                        },
                    ))
                    .await?
                    .json()
                    .await?;
                    RECORD_ID.store(resp.id, Ordering::Relaxed);
                    Ok(RecordUpdateState {
                        best: resp.new_best,
                        improvement: resp.improvement,
                        gain_exp: resp.exp_delta as f32,
                        new_rks: Some(resp.new_rks),
                    })
                })
            }));
            if is_unlock {
                #[cfg(not(feature = "video"))]
                {
                    warn!("this build does not support unlock video.");
                    LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn, save_fn, Some(preload))
                        .await
                        .map(|it| NextScene::Overlay(Box::new(it)))
                }
                #[cfg(feature = "video")]
                {
                    let chart = get_data_mut().charts.iter_mut().find(|it| it.local_path == local_path).unwrap();
                    if !chart.played_unlock {
                        chart.played_unlock = true;
                        save_data()?;
                    }

                    UnlockScene::new(mode, info, config, fs, player, upload_fn, update_fn, save_fn, Some(preload))
                        .await
                        .map(|it| NextScene::Overlay(Box::new(it)))
                }
            } else {
                LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn, save_fn, Some(preload))
                    .await
                    .map(|it| NextScene::Overlay(Box::new(it)))
            }
        })))
    }

    fn is_owner(&self) -> bool {
        self.info.id.is_none()
            || (self.info.created.is_some() && self.info.uploader.as_ref().map(|it| it.id) == get_data().me.as_ref().map(|it| it.id))
    }

    fn hide_side(&mut self, rt: f32) {
        self.side_enter_time = -rt;
    }

    fn side_chart_info(&mut self, ui: &mut Ui, rt: f32) -> Result<()> {

        let accent     = crate::theme::FIREFLY_PINK_DEEP;
        let body_bg    = Color::new(0.165, 0.110, 0.180, 1.);
        let sidebar_bg = Color::new(0.08, 0.08, 0.08, 1.);
        let dark_text  = Color::new(0.984, 0.973, 0.886, 1.);
        let muted_text = Color::new(0.65, 0.65, 0.65, 1.);
        let border_c   = Color::new(0.28, 0.28, 0.28, 1.);
        let sel_bg     = Color::new(accent.r, accent.g, accent.b, 0.14);

        let sw = 2.0_f32;
        let sh = ui.top * 2.;

        let title_h = 0.10_f32;
        let bar_h   = 0.095_f32;
        let nav_w   = 0.44_f32;
        let content_w = sw - nav_w;
        let content_h = sh - title_h - bar_h;

        ui.fill_rect(Rect::new(0., 0., sw, sh), body_bg);

        let title_r = Rect::new(0., 0., sw, title_h);
        ui.fill_rect(title_r, accent);
        ui.text(tl!("edit-chart"))
            .pos(0.05, title_r.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.52)
            .color(WHITE)
            .draw();

        let nav_r = Rect::new(0., title_h, nav_w, sh - title_h - bar_h);
        ui.fill_rect(nav_r, sidebar_bg);
        ui.fill_rect(Rect::new(nav_w - 0.002, nav_r.y, 0.002, nav_r.h), border_c);

        let tab_h = (nav_r.h / EDIT_TAB_COUNT as f32).min(0.10);
        let info_page = self.info_page.as_mut().unwrap();
        ui.scope(|ui| {
            ui.dx(0.);
            ui.dy(title_h);
            for i in 0..EDIT_TAB_COUNT {
                let r = Rect::new(0., i as f32 * tab_h, nav_w, tab_h);
                let selected = info_page.selected_tab == i;
                if selected {
                    ui.fill_rect(r, sel_bg);
                    ui.fill_rect(Rect::new(r.x, r.y, 0.005, r.h), accent);
                }
                info_page.tab_btns[i].render_shadow(ui, r, rt, |ui, _path| {
                    let label = EDIT_TAB_LABELS[i];
                    ui.text(label)
                        .pos(r.x + 0.028, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.48)
                        .color(if selected { accent } else { dark_text })
                        .draw();
                    if i + 1 < EDIT_TAB_COUNT {
                        ui.fill_rect(Rect::new(r.x + 0.012, r.bottom() - 0.001, r.w - 0.024, 0.001), border_c);
                    }
                });
            }
        });

        let bar_y = sh - bar_h;
        ui.fill_rect(Rect::new(0., bar_y, sw, 0.002), border_c);
        ui.fill_rect(Rect::new(0., bar_y, sw, bar_h), body_bg);

        let btn_h   = bar_h - 0.022;
        let btn_w   = 0.22_f32;
        let btn_gap = 0.012_f32;
        let btn_pad = 0.018_f32;
        let by = bar_y + (bar_h - btn_h) / 2.;
        let mut bx = sw - btn_pad;

        bx -= btn_w;
        let save_r = Rect::new(bx, by, btn_w, btn_h);
        self.btn_edit_save.render_shadow(ui, save_r, rt, |ui, path| {
            ui.fill_path(&path, accent);
            ui.text(tl!("edit-save"))
                .pos(save_r.center().x, save_r.center().y)
                .anchor(0.5, 0.5).no_baseline().size(0.44).color(WHITE).draw();
        });

        bx -= btn_gap + btn_w;
        let cancel_r = Rect::new(bx, by, btn_w, btn_h);
        self.btn_edit_cancel.render_shadow(ui, cancel_r, rt, |ui, _path| {
            let p = cancel_r.rounded(0.008);
            ui.fill_path(&p, body_bg);
            ui.stroke_path(&p, 0.005, border_c);
            ui.text(tl!("edit-cancel"))
                .pos(cancel_r.center().x, cancel_r.center().y)
                .anchor(0.5, 0.5).no_baseline().size(0.44).color(dark_text).draw();
        });

        let is_owner = self.is_owner();
        let online   = self.info.id.is_some();

        let lbtn_w = 0.26_f32;
        let mut lx = btn_pad;

        let tags_r = Rect::new(lx, by, lbtn_w, btn_h);
        self.btn_edit_tags_page.render_shadow(ui, tags_r, rt, |ui, _path| {
            let p = tags_r.rounded(0.008);
            ui.fill_path(&p, body_bg);
            ui.stroke_path(&p, 0.005, border_c);
            ui.text(tl!("edit-tags"))
                .pos(tags_r.center().x, tags_r.center().y)
                .anchor(0.5, 0.5).no_baseline().size(0.44).color(dark_text).draw();
        });
        lx += lbtn_w + btn_gap;

        if is_owner {
            let upload_r = Rect::new(lx, by, lbtn_w, btn_h);
            let upload_label = if online { tl!("edit-update") } else { tl!("edit-upload") };
            self.btn_edit_upload.render_shadow(ui, upload_r, rt, |ui, path| {
                let col = if online { accent } else { Color::new(0.18, 0.60, 0.28, 1.) };
                ui.fill_path(&path, col);
                ui.text(upload_label.as_ref())
                    .pos(upload_r.center().x, upload_r.center().y)
                    .anchor(0.5, 0.5).no_baseline().size(0.44).color(WHITE).draw();
            });
            lx += lbtn_w + btn_gap;
        }

        if is_owner && online {
            let ow_r = Rect::new(lx, by, lbtn_w, btn_h);
            if ui.button("overwrite", ow_r, tl!("edit-overwrite")) {
                request_file("overwrite");
            }
        }

        ui.ensure_touches().retain(|it| {
            !matches!(it.phase, TouchPhase::Started)
                || self.info_page.as_ref().is_some_and(|p| p.content_scroll.contains(it))
        });

        {
        let info_page = self.info_page.as_mut().unwrap();
        let tab = info_page.selected_tab;
        let mods = info_page.mods;
        let edit = &mut info_page.edit;
        let mod_btns = &mut info_page.mod_btns;
        let content_scroll = &mut info_page.content_scroll;
        content_scroll.size((content_w - 0.06, content_h));
        ui.scissor(Rect::new(nav_w, title_h, content_w, content_h), |ui| {
            ui.scope(|ui| {
                ui.dx(nav_w + 0.03);
                ui.dy(title_h);
                content_scroll.render(ui, |ui| {
                    ui.dy(0.03);
                    let label = EDIT_TAB_LABELS[tab];
                    let hr = ui.text(label)
                        .pos(0., 0.)
                        .anchor(0., 0.)
                        .no_baseline()
                        .size(0.60)
                        .color(muted_text)
                        .draw();
                    ui.dy(hr.h + 0.025);
                    ui.fill_rect(Rect::new(0., 0., content_w - 0.06, 0.002), border_c);
                    ui.dy(0.018);
                    let cw = content_w - 0.06;
                    let (w, h) = if tab == 5 {

                        const MOD_FLAGS: [Mods; 4] = [Mods::AUTOPLAY, Mods::FLIP_X, Mods::FADE_IN, Mods::FADE_OUT];
                        const MOD_TITLES: [&str; 4] = ["Autoplay", "Flip X", "Fade In", "Fade Out"];
                        const MOD_SUBS: [&str; 4] = [
                            "Auto-clear all notes.",
                            "Mirror the chart horizontally.",
                            "Notes fade in from invisible.",
                            "Notes fade out before hitting.",
                        ];
                        let item_h = 0.12_f32;
                        let sep_c = Color::new(0.28, 0.28, 0.28, 1.);
                        let mut total_h = 0_f32;
                        for (idx, &flag) in MOD_FLAGS.iter().enumerate() {
                            let on = mods.contains(flag);
                            let track_w = 0.10_f32;
                            let track_h = 0.042_f32;
                            let knob_r  = track_h * 0.42_f32;
                            let tx = cw - track_w - 0.01;
                            let ty = (item_h - track_h) / 2.;
                            let track_rect = Rect::new(tx, ty, track_w, track_h);
                            let track_path = track_rect.rounded(track_h / 2.);
                            ui.fill_path(&track_path, if on { accent } else { Color::new(0.35, 0.35, 0.35, 1.) });
                            let knob_cx = if on { tx + track_w - track_h / 2. } else { tx + track_h / 2. };
                            let knob_cy = ty + track_h / 2.;
                            ui.fill_circle(knob_cx, knob_cy, knob_r, WHITE);
                            while mod_btns.len() <= idx { mod_btns.push((DRectButton::new(), false)); }
                            mod_btns[idx].0.render_shadow(ui, Rect::new(tx - 0.01, ty - 0.01, track_w + 0.02, track_h + 0.02), rt, |_, _| {});
                            ui.text(MOD_TITLES[idx])
                                .pos(0., item_h * 0.32)
                                .anchor(0., 0.5)
                                .no_baseline()
                                .size(0.52)
                                .color(dark_text)
                                .draw();
                            ui.text(MOD_SUBS[idx])
                                .pos(0., item_h * 0.70)
                                .anchor(0., 0.5)
                                .no_baseline()
                                .size(0.36)
                                .color(muted_text)
                                .draw();
                            if idx + 1 < MOD_FLAGS.len() {
                                ui.fill_rect(Rect::new(0., item_h - 0.001, cw, 0.002), sep_c);
                            }
                            ui.dy(item_h);
                            total_h += item_h;
                        }
                        (cw, total_h)
                    } else {
                        render_chart_info_tab(ui, edit, tab, cw)
                    };
                    ui.dy(h);
                    (w, hr.h + 0.025 + 0.018 + 0.002 + h + 0.06)
                });
            });
        });
        }

        handle_chart_info_files(&mut self.info_page.as_mut().unwrap().edit);

        Ok(())
    }

    fn side_ldb(&mut self, ui: &mut Ui, rt: f32) {
        let pad = 0.03;
        let width = self.side_content.width() - pad;
        ui.dy(0.03);
        self.ldb_type_btn.render_text(
            ui,
            Rect::new(width - 0.24, 0.01, 0.23, 0.08),
            rt,
            if self.ldb_std { tl!("ldb-std") } else { tl!("ldb-score") },
            0.6,
            true,
        );
        render_ldb(
            ui,
            &tl!("ldb"),
            self.side_content.width(),
            rt,
            &mut self.ldb_scroll,
            &mut self.ldb_fader,
            &self.icons.user,
            self.ldb.as_mut().map(|it| {
                it.1.iter_mut().map(|it| LdbDisplayItem {
                    player_id: it.inner.player.id,
                    rank: it.rank,
                    score: if self.ldb_std {
                        format!("{:07}", it.inner.std_score.unwrap_or(0.) as i64)
                    } else {
                        format!("{:07}", it.inner.score)
                    },
                    alt: Some(if self.ldb_std {
                        format!("{}ms", (it.inner.std.unwrap_or(0.) * 1000.) as i32)
                    } else {
                        format!("{:.2}%", it.inner.accuracy * 100.)
                    }),
                    btn: &mut it.btn,
                })
            }),
        );
    }

    fn side_info(&mut self, ui: &mut Ui, rt: f32) {
        let pad = 0.03;
        ui.dx(pad);
        ui.dy(0.03);
        let width = self.side_content.width() - pad;
        self.info_scroll.size((width - pad, ui.top * 2. - 0.06));
        self.info_scroll.render(ui, |ui| {
            let mut h = 0.;
            macro_rules! dy {
                ($e:expr) => {{
                    let dy = $e;
                    h += dy;
                    ui.dy(dy);
                }};
            }
            let mw = width - pad * 3.;
            if self.info.id.is_some() {
                let r = Rect::new(0.03, 0., mw, 0.12).nonuniform_feather(-0.03, -0.01);
                self.open_web_btn.render_text(ui, r, rt, ttl!("open-in-web"), 0.6, true);
                dy!(r.h + 0.04);
            }
            if let Some(uploader) = &self.info.uploader {
                let c = 0.06;
                let s = 0.05;
                let r = ui.avatar(c, c, s, rt, UserManager::opt_avatar(uploader.id, &self.icons.user));
                self.uploader_btn.set(ui, Rect::new(c - s, c - s, s * 2., s * 2.));
                if let Some((name, color)) = UserManager::name_and_color(uploader.id) {
                    ui.text(name)
                        .pos(r.right() + 0.02, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .max_width(width - 0.15)
                        .size(0.6)
                        .color(color)
                        .draw();
                }
                dy!(0.14);
            }
            let mut item = |title: Cow<'_, str>, content: Cow<'_, str>| {
                dy!(ui.text(title).size(0.4).color(semi_white(0.7)).draw().h + 0.02);
                dy!(ui.text(content).pos(pad, 0.).size(0.6).multiline().max_width(mw).draw().h + 0.03);
            };
            item(tl!("info-name"), self.info.name.as_str().into());
            item(tl!("info-composer"), self.info.composer.as_str().into());
            item(tl!("info-charter"), self.info.charter.as_str().into());
            item(tl!("info-difficulty"), format!("{} ({:.1})", self.info.level, self.info.difficulty).into());
            item(tl!("info-desc"), self.info.intro.as_str().into());
            if let Some(entity) = &self.entity {
                item(tl!("info-rating"), entity.rating.map_or(Cow::Borrowed("NaN"), |r| format!("{:.2} / 5.00", r * 5.).into()));
                item(
                    tl!("info-type"),
                    format!(
                        "{}{}",
                        if entity.reviewed { tl!("reviewed") } else { tl!("unreviewed") },
                        match (entity.stable, entity.ranked) {
                            (true, true) => ttl!("chart-ranked"),
                            (true, false) => ttl!("chart-special"),
                            (false, _) => ttl!("chart-unstable"),
                        }
                    )
                    .into(),
                );
                item(tl!("info-tags"), entity.tags.iter().map(|it| format!("#{it}")).join(" ").into());
            }
            if let Some(id) = self.info.id {
                item("ID".into(), id.to_string().into());
            }
            (width, h)
        });
    }

    fn side_mods(&mut self, ui: &mut Ui) {
        let w = self.side_content.width();
        let pad = 0.045_f32;
        let pink = crate::theme::FIREFLY_PINK;
        let pink_deep = crate::theme::FIREFLY_PINK_DEEP;
        let cream = crate::theme::cream_text(1.);
        let muted = semi_white(0.55);

        ui.fill_rect(Rect::new(0., 0., w, ui.top * 2.), semi_black(0.45));

        let header_y = 0.10_f32;
        ui.text("MOD").pos(pad, header_y).anchor(0., 0.5).no_baseline().size(0.72).color(pink).draw();
        let ex = ui
            .text("← EXIT")
            .pos(w - pad, header_y)
            .anchor(1., 0.5)
            .no_baseline()
            .size(0.5)
            .color(cream)
            .draw();
        self.mod_exit_btn.set(ui, Rect::new(ex.x - 0.02, ex.y - 0.02, ex.w + 0.04, ex.h + 0.04));

        let item_h = 0.14_f32;
        let mut y = 0.26_f32;
        for (i, &flag) in MOD_FLAGS.iter().enumerate() {
            let on = self.mods.contains(flag);
            let row = Rect::new(pad, y, w - pad * 2., item_h);
            ui.text(MOD_TITLES[i])
                .pos(row.x, row.center().y - 0.025)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.56)
                .color(cream)
                .draw();
            ui.text(MOD_SUBS[i])
                .pos(row.x, row.center().y + 0.032)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.34)
                .color(muted)
                .draw();
            let pill_w = 0.14_f32;
            let pill_h = 0.075_f32;
            let pill = Rect::new(row.right() - pill_w, row.center().y - pill_h / 2., pill_w, pill_h);
            let (pill_bg, pill_tc) = if on {
                (pink_deep, WHITE)
            } else {
                (Color::new(0.92, 0.92, 0.93, 1.), Color::new(0.1, 0.1, 0.12, 1.))
            };
            ui.fill_path(&pill.rounded(0.014), pill_bg);
            ui.text(if on { "ON" } else { "OFF" })
                .pos(pill.center().x, pill.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.5)
                .color(pill_tc)
                .draw();
            self.mod_toggle_btns[i].set(ui, pill.feather(0.012));
            ui.fill_rect(Rect::new(row.x, row.bottom(), row.w, 0.0015), semi_white(0.12));
            y += item_h + 0.02;
        }
    }

    fn save_edit(&mut self) {
        let Some(page) = &self.info_page else { unreachable!() };
        let edit = &page.edit;
        let info = edit.info.clone();
        let path = self.local_path.clone().unwrap();
        let edit = edit.clone();
        let is_owner = self.is_owner();
        let def_illu = self.illu.texture.1.clone();
        self.save_task = Some(Task::new(async move {
            let dir = xcsim_core::dir::Dir::new(format!("{}/{path}", dir::charts()?))?;
            let patches = edit.to_patches().await.with_context(|| tl!("edit-load-file-failed"))?;
            if !is_owner && patches.contains_key(&info.chart) {
                bail!(tl!("edit-downloaded"));
            }
            for (name, bytes) in patches.into_iter() {
                dir.create(name)?.write_all(&bytes)?;
            }
            let _ = std::fs::remove_file(thumbnail_path(&path)?);
            load_local_tuple(&path, def_illu, info).await
        }));
    }

    fn update_chart_info(&self) -> Result<()> {
        Self::global_update_chart_info(self.local_path.as_ref().unwrap(), self.info.clone())
    }

    fn global_update_chart_info(local_path: &str, info: BriefChartInfo) -> Result<()> {
        let _ = std::fs::remove_file(thumbnail_path(local_path)?);
        get_data_mut().charts[get_data().find_chart_by_path(local_path).unwrap()].info = info;
        NEED_UPDATE.store(true, Ordering::Relaxed);
        save_data()?;
        Ok(())
    }

    fn load_tuple(&mut self, (local_path, info, preview, illu): LocalTuple) -> Result<()> {
        self.local_path = Some(local_path);
        if let Some(preview) = &mut self.preview {
            preview.pause()?;
        }
        self.preview = Some(create_music(preview)?);
        self.chart_music = info.music.clone();
        self.chart_illu = info.illustration.clone();
        self.info = info.into();
        self.illu = illu;
        self.update_chart_info()?;

        Ok(())
    }

    fn matches_ref(&self, r: &ChartRef) -> bool {
        r.matches((self.local_path.as_deref(), self.info.id))
    }

    fn to_chart_ref(&self) -> Option<ChartRef> {
        Some(if let Some(local) = &self.local_path {
            ChartRef::Local(local.clone())
        } else {
            match self.entity.clone() {
                Some(entity) => entity.into(),
                None => {
                    show_message(tl!("still-loading")).error();
                    return None;
                }
            }
        })
    }

    fn toggle_in(&mut self, uuid: Uuid) {
        let data = get_data();
        let mut col = data.collection_info(&uuid).as_ref().clone();
        if col.id.is_some() && self.info.id.is_none() {
            Dialog::simple(ttl!("favorites-online-only", "charts" => &self.info.name)).show();
            return;
        }

        let should_upload = col.id.is_some() && !get_data().config.offline_mode;
        let index = col.charts.iter().position(|it| self.matches_ref(it));
        if let Some(index) = index {
            col.charts.remove(index);
        } else if let Some(chart) = self.to_chart_ref() {
            col.charts.push(chart);
            if !should_upload {
                show_message(tl!("fav-added")).ok();
            }
        } else {
            return;
        }
        let col_id = col.id;
        data.set_collection_info(&uuid, col).unwrap();
        FAV_UPDATED.store(true, Ordering::SeqCst);
        if !should_upload {
            return;
        }

        if let Some(col_id) = col_id {
            let id = self.info.id.unwrap();
            self.toggle_fav_task = Some(Task::new(async move {
                let resp: Collection = recv_raw(Client::request(Method::PATCH, format!("/collection/{col_id}")).json(&CollectionPatch::Toggle(id)))
                    .await?
                    .json()
                    .await?;
                let added = resp.charts.iter().any(|it| it.id == id);
                Ok((resp, added))
            }));
        }
    }

    fn get_fav_menu_options(&mut self) -> Vec<String> {
        let data = get_data();
        let mut options = Vec::new();
        self.fav_menu_options.clear();
        for uuid in data.collection_uuids() {
            let col = data.collection_info(uuid);
            if !col.is_owned() {
                continue;
            }
            self.fav_menu_options.push(*uuid);
            let contains = col.charts.iter().any(|it| self.matches_ref(it));
            options.push(format!("{} {}", if contains { '\u{2713}' } else { ' ' }, col.name));
        }
        options
    }
}

impl Scene for SongScene {
    fn on_result(&mut self, tm: &mut TimeManager, res: Box<dyn Any>) -> Result<()> {
        let res = match res.downcast::<SimpleRecord>() {
            Err(res) => res,
            Ok(rec) => {
                self.fade_start = tm.now() as f32 + fade_in_time().unwrap_or_default();
                if self.my_rate_score == Some(0) && thread_rng().gen_ratio(2, 5) {
                    self.rate_dialog.enter(tm.real_time() as _);
                }
                self.record.as_mut().map(|it| it.update(rec.as_ref()));
                self.load_ldb();
                return Ok(());
            }
        };
        let res = match res.downcast::<anyhow::Error>() {
            Ok(error) => {
                show_error(error.context(tl!("load-chart-failed")));
                return Ok(());
            }
            Err(res) => res,
        };
        let _res = match res.downcast::<Option<f32>>() {
            Ok(offset) => {
                if let Some(offset) = *offset {
                    let dir = format!("{}/{}", dir::charts()?, self.local_path.as_ref().unwrap().replace(':', "_"));
                    let path = std::path::Path::new(&dir);
                    if !path.exists() {
                        std::fs::create_dir_all(path)?;
                    }
                    let dir = xcsim_core::dir::Dir::new(dir)?;
                    match self.chart_type {
                        ChartType::Integrated => {
                            dir.create("offset")?.write_all(&offset.to_be_bytes())?;
                            if let Ok(Some(info)) = ASSET_CHART_INFO.lock().as_deref_mut() {
                                info.offset = offset;
                            }
                        }
                        _ => {
                            let mut info: ChartInfo = serde_yaml::from_reader(&dir.open("info.yml")?)?;
                            info.offset = offset;
                            dir.create("info.yml")?.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
                            let path = thumbnail_path(self.local_path.as_ref().unwrap())?;
                            if path.exists() {
                                std::fs::remove_file(path)?;
                            }
                        }
                    }
                    show_message(tl!("edit-saved")).ok();
                }
                return Ok(());
            }
            Err(res) => res,
        };
        Ok(())
    }

    fn pause(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(preview) = &mut self.preview {
            preview.pause()?;
        }
        Ok(())
    }

    fn resume(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(preview) = &mut self.preview {
            preview.play()?;
        }
        Ok(())
    }

    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.first_in {
            self.first_in = false;
            tm.seek_to(-fade_in_time().unwrap_or_default() as _);
            self.load_ldb();
        }
        if let Some(music) = &mut self.preview {
            music.seek_to(0.)?;
            music.play()?;
        }
        self.update_menu();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if self.scene_task.is_some()
            || self.save_task.is_some()
            || self.upload_task.is_some()
            || self.review_task.is_some()
            || self.edit_tags_task.is_some()
            || self.rate_task.is_some()
            || self.overwrite_task.is_some()
            || self.update_cksum_task.is_some()
            || self.toggle_fav_task.is_some()
        {
            return Ok(true);
        }
        if let Some(dl) = &mut self.downloading {
            if dl.touch(touch, t) {
                let atomicity = dl.atomicity.clone();
                let _guard = atomicity.lock().unwrap();
                self.downloading = None;
                return Ok(true);
            }
            return Ok(false);
        }
        let rt = tm.real_time() as f32;
        if self.tags.touch(touch, rt) {
            return Ok(true);
        }
        if self.rate_dialog.touch(touch, rt) {
            return Ok(true);
        }
        if self.menu.showing() {
            self.menu.touch(touch, t);
            return Ok(true);
        }
        if self.fav_menu.showing() {
            self.fav_menu.touch(touch, t);
            return Ok(true);
        }
        if self.side_enter_time.is_finite() {
            if self.side_enter_time > 0. && tm.real_time() as f32 > self.side_enter_time + edit_transit().unwrap_or_default() {
                if touch.position.x < 1. - self.side_content.width() && touch.phase == TouchPhase::Started && self.save_task.is_none() {
                    if matches!(self.side_content, SideContent::Edit) && self.info_page.as_ref().is_some_and(|it| it.edit.updated) {
                        confirm_dialog(tl!("warn"), tl!("cancel-not-saved"), self.confirm_cancel_edit.clone());
                    } else {
                        self.hide_side(rt);
                    }
                    return Ok(true);
                }
                match self.side_content {
                    SideContent::Edit => {

                        if let Some(page) = &mut self.info_page {
                            for i in 0..EDIT_TAB_COUNT {
                                if page.tab_btns[i].touch(touch, rt) {
                                    if page.selected_tab != i {
                                        page.selected_tab = i;
                                        page.content_scroll.y_scroller.offset = 0.;
                                    }
                                    return Ok(true);
                                }
                            }
                        }

                        if let Some(page) = &mut self.info_page {
                            if page.touch_scroll(touch, t) {
                                return Ok(true);
                            }
                        }

                        if self.info_page.as_ref().is_some_and(|p| p.selected_tab == 5) {
                            const MOD_FLAGS: [Mods; 4] = [Mods::AUTOPLAY, Mods::FLIP_X, Mods::FADE_IN, Mods::FADE_OUT];
                            let page = self.info_page.as_mut().unwrap();
                            for (i, (btn, _)) in page.mod_btns.iter_mut().enumerate() {
                                if btn.touch(touch, rt) {
                                    if let Some(&flag) = MOD_FLAGS.get(i) {
                                        page.mods.toggle_mod(flag);
                                        self.mods = page.mods;
                                        if let Some(idx) = self.local_path.as_deref().and_then(|p| get_data().find_chart_by_path(p)) {
                                            let chart = &mut get_data_mut().charts[idx];
                                            if chart.mods != self.mods {
                                                chart.mods = self.mods;
                                                save_data()?;
                                            }
                                        }
                                    }
                                    return Ok(true);
                                }
                            }
                        }

                        if self.btn_edit_save.touch(touch, rt) {
                            self.save_edit();
                            return Ok(true);
                        }
                        if self.btn_edit_cancel.touch(touch, rt) {
                            if self.info_page.as_ref().is_some_and(|it| it.edit.updated) {
                                confirm_dialog(tl!("warn"), tl!("cancel-not-saved"), self.confirm_cancel_edit.clone());
                            } else {
                                self.hide_side(rt);
                            }
                            return Ok(true);
                        }
                        if self.btn_edit_tags_page.touch(touch, rt) {
                            let tags = self.info_page.as_ref().unwrap().edit.info.tags.clone();
                            self.tags.set(tags);
                            self.tags.enter(rt);
                            return Ok(true);
                        }
                        let is_owner = self.is_owner();
                        if is_owner && self.btn_edit_upload.touch(touch, rt) {
                            if self.info_page.as_ref().unwrap().edit.updated && !UPLOAD_NOT_SAVED.load(Ordering::SeqCst) {
                                Dialog::simple(tl!("upload-not-saved"))
                                    .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                                    .listener(|_dialog, pos| {
                                        if pos == 1 { UPLOAD_NOT_SAVED.store(true, Ordering::SeqCst); }
                                        false
                                    })
                                    .show();
                            } else {
                                let path = self.local_path.as_ref().unwrap();
                                if get_data().me.is_none() {
                                    show_message(tl!("upload-login-first"));
                                } else if path.starts_with(':') {
                                    show_message(tl!("upload-builtin"));
                                } else {
                                    self.update_cksum_passed = None;
                                    Dialog::plain(tl!("upload-rules"), tl!("upload-rules-content"))
                                        .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                                        .listener(|_dialog, pos| {
                                            if pos == 1 { CONFIRM_UPLOAD.store(true, Ordering::SeqCst); }
                                            false
                                        })
                                        .show();
                                }
                            }
                            return Ok(true);
                        }
                    }
                    SideContent::Leaderboard => {
                        if self.ldb_type_btn.touch(touch, rt) {
                            self.ldb_std ^= true;
                            self.ldb_scroll.y_scroller.offset = 0.;
                            self.load_ldb();
                            return Ok(true);
                        }
                        if self.ldb_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                        if let Some((_, ldb)) = &mut self.ldb {
                            for item in ldb {
                                if item.btn.touch(touch) {
                                    button_hit();
                                    self.sf
                                        .goto(t, ProfileScene::new(item.inner.player.id, self.icons.user.clone(), self.rank_icons.clone()));
                                    return Ok(true);
                                }
                            }
                        }
                    }
                    SideContent::Info => {
                        if self.info_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                        if self.uploader_btn.touch(touch) {
                            button_hit();
                            self.sf.goto(
                                t,
                                ProfileScene::new(self.info.uploader.as_ref().unwrap().id, self.icons.user.clone(), self.rank_icons.clone()),
                            );
                            return Ok(true);
                        }
                        if self.open_web_btn.touch(touch, rt) {
                            open_url(&format!("https://phira.moe/chart/{}", self.info.id.unwrap()))?;
                            return Ok(true);
                        }
                    }
                    SideContent::Mods => {
                        if self.mod_exit_btn.touch(touch) {
                            button_hit();
                            self.hide_side(rt);
                            return Ok(true);
                        }
                        for (i, btn) in self.mod_toggle_btns.iter_mut().enumerate() {
                            if btn.touch(touch) {
                                button_hit();
                                self.mods.toggle_mod(MOD_FLAGS[i]);
                                if let Some(idx) = self.local_path.as_deref().and_then(|p| get_data().find_chart_by_path(p)) {
                                    let chart = &mut get_data_mut().charts[idx];
                                    if chart.mods != self.mods {
                                        chart.mods = self.mods;
                                        save_data()?;
                                    }
                                }
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            return Ok(false);
        }
        if self.si_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.desc_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.back_btn.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::PopWithResult(Box::new(false)));
            return Ok(true);
        }
        if self.scene_task.is_none() && self.next_scene.is_none() && self.play_btn.touch(touch, t) {
            if self.local_path.is_some() {
                self.launch(GameMode::Normal, false)?;
            } else {
                self.start_download()?;
            }
            return Ok(true);
        }
        if self.mod_btn.touch(touch, t) {
            button_hit();
            self.side_content = SideContent::Mods;
            self.side_enter_time = tm.real_time() as _;
            return Ok(true);
        }
        if !self.menu_options.is_empty() && self.menu_btn.touch(touch) {
            button_hit();
            self.need_show_menu = true;
            return Ok(true);
        }
        if self.fav_btn.touch(touch) {
            self.fav_long_touch.reset();
            button_hit();
            let data = get_data();
            if let Some(uuid) = data.collection_uuids().iter().find(|uuid| data.collection_info(uuid).is_default) {
                self.toggle_in(*uuid);
            }
            return Ok(true);
        }
        if self.fav_btn.long_touch(touch, t, &mut self.fav_long_touch) {
            button_hit();
            let options = self.get_fav_menu_options();
            self.fav_menu.set_options(options);
            self.need_show_fav_menu = true;
            return Ok(true);
        }
        if let Some(path) = &self.local_path {
            if self.edit_btn.touch(touch) {
                button_hit();

                if path.starts_with("@chap/") || path.starts_with("@special/") {
                    show_message("This chart is read-only and cannot be edited.").error();
                    return Ok(true);
                }
                let mut info: ChartInfo = serde_yaml::from_str(&std::fs::read_to_string(format!("{}/{path}/info.yml", dir::charts()?))?)?;
                info.id = self.info.id;
                UPLOAD_NOT_SAVED.store(false, Ordering::SeqCst);
                self.info_page = Some(ChartInfoPage::new(ChartInfoEdit::new(info), self.mods));
                self.side_content = SideContent::Edit;
                self.side_enter_time = tm.real_time() as _;
                return Ok(true);
            }
        }
        if self.info.id.is_some() && self.ldb_btn.touch(touch) {
            button_hit();
            self.side_content = SideContent::Leaderboard;
            self.side_enter_time = tm.real_time() as _;
        }
        if self.info_btn.touch(touch) {
            button_hit();
            if let Some(uploader) = &self.info.uploader {
                UserManager::request(uploader.id);
            }
            self.side_content = SideContent::Info;
            self.side_enter_time = tm.real_time() as _;
            return Ok(true);
        }

        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.menu.update(t);
        self.fav_menu.update(t);
        self.si_scroll.update(t);
        self.desc_scroll.update(t);
        self.illu.settle(t);
        let rt = tm.real_time() as f32;
        self.tags.update(rt);
        self.rate_dialog.update(rt);
        if self.tags.confirmed.take() == Some(true) {
            let mut tags = self.tags.tags.tags().to_vec();
            tags.push(self.tags.division.to_owned());
            if self.side_enter_time.is_finite() && matches!(self.side_content, SideContent::Edit) {
                let page = self.info_page.as_mut().unwrap();
                page.edit.info.tags = tags;
                page.edit.updated = true;
            } else {
                let id = self.info.id.unwrap();
                self.entity.as_mut().unwrap().tags = tags.clone();
                self.edit_tags_task = Some(Task::new(async move {
                    recv_raw(Client::post(
                        format!("/chart/{id}/edit-tags"),
                        &json!({
                            "tags": tags,
                        }),
                    ))
                    .await?;
                    Ok(())
                }));
            }
        }
        if self.rate_dialog.confirmed.take() == Some(true) {
            if let Some(id) = self.info.id {
                let score = self.rate_dialog.rate.score;
                self.rate_task = Some(Task::new(async move {
                    recv_raw(Client::post(
                        format!("/chart/{id}/rate"),
                        &json!({
                            "score": score,
                        }),
                    ))
                    .await?;
                    Ok(())
                }));
            }
        }
        if self.side_enter_time < 0. && -tm.real_time() as f32 + edit_transit().unwrap_or_default() < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-charts-failed")));
                    }
                    Ok(chart) => {
                        if let Some(chart) = chart {
                            self.entity = Some(chart.as_ref().clone());
                            if self
                                .info
                                .updated
                                .map_or(chart.updated != chart.created, |local_updated| local_updated != chart.updated)
                                && self.local_path.is_some()
                            {
                                let chart_updated = self
                                    .info
                                    .chart_updated
                                    .map_or(chart.chart_updated != chart.created, |local_updated| local_updated != chart.chart_updated);
                                confirm_dialog(
                                    tl!("need-update"),
                                    if chart_updated {
                                        tl!("need-update-content")
                                    } else {
                                        tl!("need-update-info-only-content")
                                    },
                                    Arc::clone(&self.should_update),
                                );
                            }
                        } else if let Some(local) = &self.local_path {
                            let conf = format!("{}/{}/info.yml", dir::charts()?, local);
                            let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                            info.id = None;
                            info.uploader = None;
                            info.created = None;
                            info.updated = None;
                            info.chart_updated = None;
                            serde_yaml::to_writer(File::create(conf)?, &info)?;
                            self.info = info.into();
                            self.update_chart_info()?;
                        }
                        self.update_menu();
                    }
                }
                self.load_task = None;
            }
        }
        if let Some(task) = &mut self.preview_task {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        show_error(err.context(tl!("load-preview-failed")));
                    }
                    Ok(clip) => {
                        self.preview = Some(create_music(clip)?);
                    }
                }
                self.preview_task = None;
            }
        }
        if let Some(dl) = &mut self.downloading {
            if let Some(tuple) = dl.check()? {
                self.local_path = dl.local_path.take();
                self.downloading = None;
                if let Some(tuple) = tuple {
                    self.load_tuple(tuple)?;
                }
                self.update_menu();
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        error!(?err, "failed to play");
                        *self.background.lock().unwrap() = None;
                        self.tr_start = f32::NAN;
                        let error = format!("{err:?}");
                        Dialog::plain(tl!("failed-to-play"), error)
                            .buttons(vec![tl!("play-cancel").into_owned(), tl!("play-switch-to-offline").into_owned()])
                            .listener(move |_dialog, pos| {
                                if pos == 1 {
                                    get_data_mut().config.offline_mode = true;
                                    let _ = save_data();
                                    show_message(tl!("switched-to-offline")).ok();
                                }
                                false
                            })
                            .show();
                    }
                    Ok(scene) => self.next_scene = Some(scene),
                }
                self.scene_task = None;
            }
        }
        if let Some(task) = &mut self.fetch_best_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to fetch best record");
                    }
                    Ok(rec) => {
                        self.update_record(rec)?;
                    }
                }
                self.fetch_best_task = None;
            }
        }
        if self.menu.changed() {
            let option = self.menu_options[self.menu.selected()];
            match option {
                "delete" => {
                    confirm_delete(self.should_delete.clone());
                }
                "rate" => {
                    self.rate_dialog.enter(tm.real_time() as _);
                }
                "exercise" => {
                    self.launch(GameMode::Exercise, false)?;
                }
                "offset" => {
                    self.launch(GameMode::TweakOffset, false)?;
                }
                "unlock" => {
                    self.launch(GameMode::Normal, true)?;
                }
                "review-approve" => {
                    confirm_dialog(tl!("warn"), tl!("review-approve-confirm"), Arc::clone(&self.should_review_approve));
                }
                "review-deny" => {
                    request_input("deny-reason", InputBox::new().mode(InputMode::Multiline));
                }
                "review-del" => {
                    confirm_delete(self.chart_should_delete.clone());
                }
                "review-edit-tags" => {
                    let Some(entity) = self.entity.as_ref() else {
                        show_message(tl!("review-not-loaded")).warn();
                        return Ok(());
                    };
                    self.tags.set(entity.tags.clone());
                    self.tags.enter(tm.real_time() as _);
                }
                "stabilize" => {
                    confirm_dialog(tl!("stabilize"), tl!("stabilize-warn"), Arc::clone(&self.should_stabilize));
                }
                "stabilize-approve" => {
                    confirm_dialog(tl!("warn"), tl!("stabilize-approve-confirm"), Arc::clone(&self.should_stabilize_approve));
                }
                "stabilize-approve-ranked" => {
                    confirm_dialog(tl!("warn"), tl!("stabilize-approve-confirm"), Arc::clone(&self.should_stabilize_approve_ranked));
                }
                "stabilize-comment" => {
                    request_input("stabilize-comment", InputBox::new().mode(InputMode::Multiline));
                }
                "stabilize-deny" => {
                    request_input("stabilize-deny-reason", InputBox::new().mode(InputMode::Multiline));
                }
                _ => {}
            }
        }
        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            self.next_scene = Some(NextScene::PopWithResult(Box::new(true)));
        }
        if self.fav_menu.changed() {
            let selected = self.fav_menu.selected();
            self.fav_menu.set_selected(usize::MAX);
            self.toggle_in(self.fav_menu_options[selected]);
            let _ = save_data();
            let options = self.get_fav_menu_options();
            self.fav_menu.set_options(options);
        }
        if self.chart_should_delete.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.review_task = Some(Task::new(async move {
                recv_raw(Client::delete(format!("/chart/{id}"))).await?;
                Ok(tl!("review-deleted").into_owned())
            }));
        }
        if self.should_review_approve.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.review_task = Some(Task::new(async move {
                #[derive(Deserialize)]
                struct Resp {
                    passed: bool,
                }
                let resp: Resp = recv_raw(Client::post(
                    format!("/chart/{id}/review"),
                    &json!({
                        "approve": true
                    }),
                ))
                .await?
                .json()
                .await?;
                Ok((if resp.passed { tl!("review-passed") } else { tl!("review-approved") }).into_owned())
            }));
        }
        if self.should_stabilize_approve.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.review_task = Some(Task::new(async move {
                let resp: StableR = recv_raw(Client::post(
                    format!("/chart/{id}/stabilize"),
                    &json!({
                        "kind": 0,
                    }),
                ))
                .await?
                .json()
                .await?;
                Ok((if resp.status == 0 {
                    tl!("stabilize-approved")
                } else {
                    tl!("stabilize-approved-passed")
                })
                .into())
            }));
        }
        if self.should_stabilize_approve_ranked.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.review_task = Some(Task::new(async move {
                let resp: StableR = recv_raw(Client::post(
                    format!("/chart/{id}/stabilize"),
                    &json!({
                        "kind": 1,
                    }),
                ))
                .await?
                .json()
                .await?;
                Ok((if resp.status == 0 {
                    tl!("stabilize-approved")
                } else {
                    tl!("stabilize-approved-passed")
                })
                .into())
            }));
        }
        if self.should_stabilize.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.stabilize_task = Some(Task::new(async move {
                recv_raw(Client::post(format!("/chart/{id}/req-stabilize"), &())).await?;
                Ok(())
            }));
        }
        if let Some(task) = &mut self.save_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-save-failed")));
                    }
                    Ok(tuple) => {
                        self.info_page.as_mut().unwrap().edit.updated = false;
                        self.load_tuple(tuple)?;
                        show_message(tl!("edit-saved")).duration(1.).ok();
                    }
                }
                self.save_task = None;
            }
        }
        if let Some(task) = &mut self.upload_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("upload-failed")));
                    }
                    Ok(info) => {
                        show_message(tl!("upload-success")).ok();
                        self.info = info;
                        self.update_chart_info()?;
                        self.side_enter_time = -tm.real_time() as _;
                    }
                }
                self.upload_task = None;
            }
        }
        match self.side_content {
            SideContent::Edit => {
                if let Some(p) = &mut self.info_page { p.update(t); }
            }
            SideContent::Leaderboard => {
                if self.ldb_scroll.y_scroller.pulled {
                    self.ldb_scroll.y_scroller.offset = 0.;
                    self.load_ldb();
                }
                self.ldb_scroll.update(t);
            }
            SideContent::Info => {
                self.info_scroll.update(t);
            }
            SideContent::Mods => {}
        }
        if CONFIRM_UPLOAD.fetch_and(false, Ordering::Relaxed) {
            let local_path = self.local_path.clone().unwrap();
            let id = self.info.id;
            self.update_cksum_task = Some(Task::new(async move {
                if let Some(id) = id {
                    use hex::ToHex;
                    let mut fs = fs_from_path(&local_path)?;
                    let info = xcsim_core::fs::load_info(fs.as_mut()).await?;
                    let chart = fs.load_file(&info.chart).await?;
                    let cksum: String = Sha256::digest(&chart).encode_hex();
                    #[derive(Deserialize)]
                    struct VerifyR {
                        ok: bool,
                    }
                    let resp: VerifyR = recv_raw(Client::get(format!("/chart/{id}/verify-cksum?checksum={cksum}")))
                        .await?
                        .json()
                        .await?;
                    Ok(resp.ok)
                } else {
                    Ok(true)
                }
            }));
        }
        if let Some(task) = &mut self.update_cksum_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("upload-failed")));
                    }
                    Ok(ok) => {
                        if ok {
                            CONFIRM_CKSUM.store(true, Ordering::Relaxed);
                        } else {
                            Dialog::simple(tl!("upload-confirm-clear-ldb"))
                                .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                                .listener(move |_dialog, pos| {
                                    if pos == 1 {
                                        CONFIRM_CKSUM.store(true, Ordering::Relaxed);
                                    }
                                    false
                                })
                                .show();
                        }
                    }
                }
                self.update_cksum_task = None;
            }
        }
        if CONFIRM_CKSUM.fetch_and(false, Ordering::Relaxed) {
            let path = self.local_path.clone().unwrap();
            let info = self.info.clone();
            self.upload_task = Some(Task::new(async move {
                let root = format!("{}/{path}", dir::charts()?);
                let root = Path::new(&root);
                let chart_bytes = compress_folder(root)?;
                let file = Client::upload_file("chart.zip", chart_bytes)
                    .await
                    .with_context(|| tl!("upload-chart-failed"))?;
                if let Some(id) = info.id {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Resp {
                        updated: DateTime<Utc>,
                        chart_updated: DateTime<Utc>,
                    }
                    let resp: Resp = recv_raw(Client::request(Method::PATCH, format!("/chart/{id}")).json(&json!({
                        "file": file,
                        "created": info.created.unwrap(),
                    })))
                    .await?
                    .json()
                    .await?;
                    let conf = root.join("info.yml");
                    let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                    info.updated = Some(resp.updated);
                    info.chart_updated = Some(resp.chart_updated);
                    serde_yaml::to_writer(File::create(conf)?, &info)?;
                    Ok(info.into())
                } else {
                    #[derive(Deserialize)]
                    struct Resp {
                        id: i32,
                        created: DateTime<Utc>,
                    }
                    let resp: Resp = recv_raw(Client::post(
                        "/chart/upload",
                        &json!({
                            "file": file,
                        }),
                    ))
                    .await?
                    .json()
                    .await?;
                    let conf = root.join("info.yml");
                    let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                    info.id = Some(resp.id);
                    info.created = Some(resp.created);
                    info.updated = Some(resp.created);
                    info.chart_updated = Some(resp.created);
                    info.uploader = Some(get_data().me.as_ref().unwrap().id);
                    serde_yaml::to_writer(File::create(conf)?, &info)?;
                    Ok(info.into())
                }
            }));
        }
        if let Some(task) = &mut self.ldb_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("ldb-load-failed")));
                    }
                    Ok(items) => {
                        let rank = get_data()
                            .me
                            .as_ref()
                            .and_then(|me| items.iter().find(|it| it.inner.player.id == me.id).map(|it| it.rank));
                        for item in &items {
                            UserManager::request(item.inner.player.id);
                        }
                        self.ldb = Some((rank, items));
                        self.ldb_fader.sub(tm.real_time() as _);
                    }
                }
                self.ldb_task = None;
            }
        }
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "deny-reason" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        recv_raw(Client::post(
                            format!("/chart/{id}/review"),
                            &json!({
                                "approve": false,
                                "reason": text,
                            }),
                        ))
                        .await?;
                        Ok(tl!("review-denied").into_owned())
                    }));
                }
                "stabilize-comment" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        recv_raw(Client::post(
                            format!("/chart/{id}/stabilize-comment"),
                            &json!({
                                "comment": text,
                            }),
                        ))
                        .await?;
                        Ok(tl!("stabilize-commented").into())
                    }));
                }
                "stabilize-deny-reason" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        let resp: StableR = recv_raw(Client::post(
                            format!("/chart/{id}/stabilize"),
                            &json!({
                                "kind": -1,
                                "reason": text,
                            }),
                        ))
                        .await?
                        .json()
                        .await?;
                        Ok((if resp.status == 0 {
                            tl!("stabilize-denied")
                        } else {
                            tl!("stabilize-denied-passed")
                        })
                        .into())
                    }));
                }
                _ => return_input(id, text),
            }
        }
        if let Some((id, file)) = take_file() {
            if id == "overwrite" {
                self.overwrite_from = Some(file);
                CONFIRM_OVERWRITE.store(false, Ordering::SeqCst);
                Dialog::simple(tl!("edit-overwrite-confirm"))
                    .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                    .listener(move |_dialog, pos| {
                        if pos == 1 {
                            CONFIRM_OVERWRITE.store(true, Ordering::SeqCst);
                        }
                        false
                    })
                    .show();
            } else {
                return_file(id, file);
            }
        }
        if CONFIRM_OVERWRITE.fetch_and(false, Ordering::Relaxed) {
            let path = self.overwrite_from.take().unwrap();
            let local_path = self.local_path.clone().unwrap();
            let def_illu = self.illu.texture.1.clone();
            let chart_id = self.info.id.unwrap();
            let owner = self.info.uploader.as_ref().unwrap().id;
            self.overwrite_task = Some(Task::new(async move {
                let (dir, id) = gen_custom_dir()?;
                let to_path = format!("{}/{}/", dir::charts()?, local_path);
                let file = File::open(path).context("cannot open file")?;
                if let Err(err) = import_chart_to(&dir, format!("custom/{id}"), file).await {
                    std::fs::remove_dir_all(dir)?;
                    return Err(err);
                }
                let mut fs = xcsim_core::fs::fs_from_file(&dir)?;
                let mut info = xcsim_core::fs::load_info(fs.as_mut()).await?;
                drop(fs);
                info.id = Some(chart_id);
                info.uploader = Some(owner);
                serde_yaml::to_writer(File::create(dir.join("info.yml"))?, &info)?;

                std::fs::remove_dir_all(&to_path)?;
                std::fs::rename(&dir, &to_path)?;

                load_local_tuple(&local_path, def_illu, info).await
            }));
        }
        if let Some(task) = &mut self.overwrite_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-overwrite-failed")));
                    }
                    Ok(tuple) => {
                        self.load_tuple(tuple)?;
                        show_message(tl!("edit-overwrite-success")).ok();
                    }
                }
                self.overwrite_task = None;
            }
        }
        if let Some(task) = &mut self.review_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("review-action-failed")));
                    }
                    Ok(msg) => {
                        show_message(msg).ok();
                    }
                }
                self.review_task = None;
            }
        }
        if let Some(task) = &mut self.stabilize_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("stabilize-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("stabilize-requested")).ok();
                    }
                }
                self.review_task = None;
            }
        }
        if let Some(task) = &mut self.edit_tags_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("review-edit-tags-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("review-edit-tags-done")).ok();
                    }
                }
                self.edit_tags_task = None;
            }
        }
        if let Some(task) = &mut self.rate_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("rate-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("rate-done")).ok();
                    }
                }
                self.rate_dialog.dismiss(rt);
                self.rate_task = None;
            }
        }
        if self.should_update.fetch_and(false, Ordering::Relaxed) {
            self.start_download()?;
        }
        if let Some(task) = &mut self.my_rating_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to fetch my rating status");
                    }
                    Ok(score) => {
                        self.rate_dialog.rate.score = score;
                        self.my_rate_score = Some(score);
                    }
                }
                self.my_rating_task = None;
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Ok(scene) => self.next_scene = Some(scene),
                    Err(err) => {
                        if format!("{err:?}").contains(xcsim_core::blacklist::BLACKLIST_ERR) {

                            Dialog::plain(tl!("respack-copyright-title"), tl!("respack-copyright-body")).show();
                            get_data_mut().respack_id = 0;
                            save_data()?;
                        } else {
                            return Err(err);
                        }
                    }
                }
                self.scene_task = None;
            }
        }
        if let Some(task) = &mut self.toggle_fav_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err);
                    }
                    Ok((col, added)) => {
                        let data = get_data();
                        if let Some(uuid) = data.collection_uuids().iter().find(|it| data.collection_info(it).id == Some(col.id)) {
                            let uuid = *uuid;
                            let local = data.collection_info(&uuid);
                            data.set_collection_info(&uuid, local.merge(&col))?;
                        }
                        if added {
                            show_message(tl!("fav-added")).ok();
                        }
                        FAV_UPDATED.store(true, Ordering::SeqCst);
                    }
                }
                self.toggle_fav_task = None;
            }
        }
        if self.confirm_cancel_edit.swap(false, Ordering::Relaxed) {
            self.hide_side(rt);
        }
        if self.tr_start.is_nan() && self.background.lock().unwrap().is_some() && !get_data().prefer_reduced_motion {
            self.tr_start = rt;
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;
        let top = ui.top;

        let pink = crate::theme::FIREFLY_PINK;
        let pink_deep = crate::theme::FIREFLY_PINK_DEEP;
        let cream = crate::theme::cream_text(1.);
        let muted = semi_white(0.6);
        let panel_c = Color::new(0.10, 0.07, 0.12, 0.62);

        ui.fill_rect(ui.screen_rect(), (*self.illu.texture.1, ui.screen_rect()));
        ui.fill_rect(ui.screen_rect(), semi_black(0.62));

        let bar_h = 0.13_f32;
        let bar_y = -top;
        let bar_cy = bar_y + bar_h * 0.5;

        let back_r = Rect::new(-0.97, bar_y + 0.025, 0.22, bar_h - 0.05);
        self.back_btn.set(ui, back_r);
        ui.text("← EXIT")
            .pos(back_r.x + 0.03, bar_cy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.5)
            .color(cream)
            .draw();

        let wm = ui.text("XC-SIM").pos(0., bar_cy).anchor(0.5, 0.5).no_baseline().size(0.74).color(pink).draw();
        ui.text("\\\\")
            .pos(wm.right() + 0.02, bar_cy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.74)
            .color(semi_white(0.5))
            .draw();

        let menu_r = Rect::new(0.86, bar_y + 0.02, 0.11, bar_h - 0.04);
        self.menu_btn.set(ui, menu_r);
        ui.text("⋯")
            .pos(menu_r.center().x, bar_cy)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.74)
            .color(if self.menu_options.is_empty() { semi_white(0.4) } else { cream })
            .draw();
        if self.need_show_menu {
            self.need_show_menu = false;
            self.menu.set_bottom(true);
            self.menu.set_selected(usize::MAX);
            self.menu.show(ui, t, Rect::new(0.62, bar_y + bar_h + 0.02, 0.35, 0.5));
        }

        let alpha = fade_in_time().map_or(1., |tt| ((t - self.fade_start) / tt).clamp(-1., 0.) + 1.);
        ui.alpha::<Result<()>>(alpha, |ui| {
            let off = Rect::new(-99., -99., 0.001, 0.001);
            self.info_btn.set(ui, off);
            self.fav_btn.set(ui, off);
            self.ldb_btn.set(ui, off);

            let content_y = bar_y + bar_h;

            let local = self.local_path.is_some();
            let set_icon = self.icons.settings.clone();
            let play_label = if local { tl!("start-play").into_owned() } else { tl!("download").into_owned() };
            let score = self.record.as_ref().map(|it| it.score).unwrap_or_default();
            let accuracy = self.record.as_ref().map(|it| it.accuracy).unwrap_or_default();
            let grade = self.record.as_ref().map_or(0, |it| icon_index(it.score as _, it.full_combo));

            let card = Rect::new(-0.95, content_y + 0.02, 0.90, 0.40);
            let crad = 0.022_f32;
            ui.fill_path(&card.rounded(crad), semi_black(0.4));
            ui.fill_path(&card.rounded(crad), (*self.illu.texture.1, card, ScaleType::CropCenter));
            for i in 0..10 {
                let f0 = i as f32 / 10.;
                let f1 = (i + 1) as f32 / 10.;
                ui.fill_rect(
                    Rect::new(card.x + crad, card.bottom() - 0.18 + 0.18 * f0, card.w - 2. * crad, 0.18 * (f1 - f0) + 0.002),
                    semi_black(0.78 * f1 * f1),
                );
            }
            ui.text(&self.info.name)
                .pos(card.x + 0.035, card.bottom() - 0.03)
                .anchor(0., 1.)
                .no_baseline()
                .size(0.86)
                .max_width(card.w - 0.07)
                .color(WHITE)
                .draw();

            let sip = Rect::new(card.x, card.bottom() + 0.03, card.w, top - card.bottom() - 0.09);
            ui.fill_path(&sip.rounded(0.016), panel_c);
            ui.text(tl!("song-info"))
                .pos(sip.x + 0.03, sip.y + 0.055)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.5)
                .color(pink)
                .draw();
            let rows: [(Cow<str>, Cow<str>); 6] = [
                (tl!("info-name"), self.info.name.as_str().into()),
                (tl!("info-composer"), self.info.composer.as_str().into()),
                (tl!("info-level"), self.info.level.as_str().into()),
                (tl!("info-charter"), self.info.charter.as_str().into()),
                (tl!("info-constant"), format!("{:.1}", self.info.difficulty).into()),
                (tl!("info-illustrator"), self.info.illustrator.as_str().into()),
            ];
            let sa = Rect::new(sip.x, sip.y + 0.095, sip.w, sip.bottom() - (sip.y + 0.095) - 0.015);
            let lx = 0.03_f32;
            let vx = 0.22_f32;
            let mw = sa.w - vx - 0.02;
            let row_h = 0.07_f32;
            self.si_scroll.size((sa.w, sa.h));
            ui.scissor(sa, |ui| {
                ui.scope(|ui| {
                    ui.dx(sa.x);
                    ui.dy(sa.y);
                    self.si_scroll.render(ui, |ui| {
                        let mut iy = 0.025_f32;
                        for (label, value) in &rows {
                            kv_row(ui, lx, vx, iy, label, value, muted, cream, mw);
                            iy += row_h;
                        }
                        (sa.w, iy + 0.01)
                    });
                });
            });

            let rcx = 0.02_f32;
            let p_r = Rect::new(rcx, content_y + 0.04, 0.13, 0.13);
            ui.fill_path(&p_r.rounded(0.03), pink);
            ui.text("P").pos(p_r.center().x, p_r.center().y).anchor(0.5, 0.5).no_baseline().size(1.1).color(WHITE).draw();
            ui.text(format!("{score:07}"))
                .pos(p_r.right() + 0.04, p_r.center().y)
                .anchor(0., 0.5)
                .no_baseline()
                .size(1.5)
                .color(WHITE)
                .draw();

            let g_r = Rect::new(rcx, p_r.bottom() + 0.035, 0.11, 0.11);
            ui.fill_rect(g_r, (*self.rank_icons[grade], g_r, ScaleType::Fit));
            ui.text(format!("{:.2}%", accuracy * 100.))
                .pos(g_r.right() + 0.04, g_r.center().y)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.8)
                .color(semi_white(0.85))
                .draw();

            let pb_y = g_r.bottom() + 0.05;
            let pb_h = 0.12_f32;
            let play_r = Rect::new(rcx, pb_y, 0.34, pb_h);
            self.play_btn.render_shadow(ui, play_r, t, |ui, path| {
                ui.fill_path(&path, pink_deep);
                ui.text("▶")
                    .pos(play_r.x + 0.04, play_r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.62)
                    .color(WHITE)
                    .draw();
                ui.text(&play_label)
                    .pos(play_r.x + 0.11, play_r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.56)
                    .max_width(play_r.w - 0.13)
                    .color(WHITE)
                    .draw();
            });
            let mod_r = Rect::new(play_r.right() + 0.03, pb_y, 0.18, pb_h);
            self.mod_btn.render_shadow(ui, mod_r, t, |ui, path| {
                ui.fill_path(&path, semi_white(0.18));
                ui.text("MOD")
                    .pos(mod_r.center().x, mod_r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.56)
                    .color(WHITE)
                    .draw();
            });
            let set_r = Rect::new(mod_r.right() + 0.03, pb_y, pb_h, pb_h);
            ui.fill_path(&set_r.rounded(0.012), semi_white(0.18));
            let isz = pb_h * 0.58;
            let sir = Rect::new(set_r.center().x - isz / 2., set_r.center().y - isz / 2., isz, isz);
            ui.fill_rect(sir, (*set_icon, sir, ScaleType::Fit, WHITE));
            self.edit_btn.set(ui, set_r);

            if self.info.id.is_some() {
                let ldb_r = Rect::new(set_r.right() + 0.03, pb_y, 0.22, pb_h);
                ui.fill_path(&ldb_r.rounded(0.014), pink_deep);
                ui.text("★")
                    .pos(ldb_r.x + 0.035, ldb_r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.6)
                    .color(WHITE)
                    .draw();
                ui.text(tl!("ldb"))
                    .pos(ldb_r.x + 0.066, ldb_r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.42)
                    .max_width(ldb_r.w - 0.085)
                    .color(WHITE)
                    .draw();
                self.ldb_btn.set(ui, ldb_r);
            }

            let fp = Rect::new(rcx, pb_y + pb_h + 0.04, 0.93, top - (pb_y + pb_h + 0.04) - 0.03);
            ui.fill_path(&fp.rounded(0.016), panel_c);
            let da = Rect::new(fp.x, fp.y + 0.03, fp.w, fp.h - 0.045);
            let flx = 0.03_f32;
            let fvx = 0.22_f32;
            let fmw = fp.w - fvx - 0.02;
            let dmw = fp.w - 0.06;
            self.desc_scroll.size((da.w, da.h));
            ui.scissor(da, |ui| {
                ui.scope(|ui| {
                    ui.dx(da.x);
                    ui.dy(da.y);
                    self.desc_scroll.render(ui, |ui| {
                        kv_row(ui, flx, fvx, 0.05, &tl!("file-audio"), &self.chart_music, muted, cream, fmw);
                        kv_row(ui, flx, fvx, 0.12, &tl!("file-illu"), &self.chart_illu, muted, cream, fmw);
                        ui.text(tl!("info-desc")).pos(flx, 0.20).anchor(0., 0.5).no_baseline().size(0.36).color(muted).draw();
                        let r = ui
                            .text(&self.info.intro)
                            .pos(0.03, 0.235)
                            .anchor(0., 0.)
                            .no_baseline()
                            .size(0.4)
                            .multiline()
                            .max_width(dmw)
                            .color(cream)
                            .draw();
                        (da.w, 0.235 + r.h + 0.03)
                    });
                });
            });

            if let Some(dl) = &mut self.downloading {
                dl.render(ui, t);
            }

            let rt = tm.real_time() as f32;
            if self.side_enter_time.is_finite() {
                let p = edit_transit().map_or(1., |t| ((rt - self.side_enter_time.abs()) / t).min(1.));
                let p = 1. - (1. - p).powi(3);
                let p = if self.side_enter_time < 0. { 1. - p } else { p };
                ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
                let w = self.side_content.width();
                let lf = f32::tween(&1.04, &(1. - w), p);
                ui.scope(|ui| {
                    ui.dx(lf);
                    ui.dy(-ui.top);
                    let r = Rect::new(-0.2, 0., 0.2 + w, ui.top * 2.);
                    ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.7), (r.right(), r.y)));

                    match self.side_content {
                        SideContent::Edit => self.side_chart_info(ui, rt),
                        SideContent::Leaderboard => {
                            self.side_ldb(ui, rt);
                            Ok(())
                        }
                        SideContent::Info => {
                            self.side_info(ui, rt);
                            Ok(())
                        }
                        SideContent::Mods => {
                            self.side_mods(ui);
                            Ok(())
                        }
                    }
                })?;
            }

            Ok(())
        })?;

        self.menu.render(ui, t, 1.);
        self.fav_menu.render(ui, t, 1.);

        if self.save_task.is_some() {
            ui.full_loading(tl!("edit-saving"), t);
        }
        if self.upload_task.is_some() {
            ui.full_loading(tl!("uploading"), t);
        }
        if self.review_task.is_some() {
            ui.full_loading(tl!("review-doing"), t);
        }
        if self.edit_tags_task.is_some()
            || self.rate_task.is_some()
            || self.overwrite_task.is_some()
            || self.update_cksum_task.is_some()
            || self.toggle_fav_task.is_some()
        {
            ui.full_loading("", t);
        }
        let rt = tm.real_time() as f32;
        self.tags.render(ui, rt);
        self.rate_dialog.render(ui, rt);

        if !self.tr_start.is_nan() {
            let p = ((rt - self.tr_start - 0.2) / 0.4).clamp(0., 1.);
            if p >= 1. {
                self.tr_start = f32::NAN;
            }
            let p = 1. - (1. - p).powi(3);
            let mut r = ui.screen_rect();
            r.y += r.h * (1. - p);
            rect_shadow(r, 0.01, 0.5);
            ui.fill_rect(r, (**self.background.lock().unwrap().as_ref().unwrap(), r));
            ui.fill_rect(r, semi_black(0.3));
        }

        self.sf.render(ui, t);

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if !self.tr_start.is_nan() {
            return NextScene::None;
        }
        if let Some(scene) = self.next_scene.take().or_else(|| self.sf.next_scene(tm.now() as _)) {
            *self.background.lock().unwrap() = None;
            if let Some(music) = &mut self.preview {
                let _ = music.pause();
            }
            scene
        } else {
            NextScene::None
        }
    }
}

pub fn compress_folder(src: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut zip = ZipWriter::new(Cursor::new(&mut bytes));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(src)?;
        if path.is_file() {
            zip.start_file_from_path(name, options)?;
            let mut f = File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
        } else if !name.as_os_str().is_empty() {
            zip.add_directory_from_path(name, options)?;
        }
    }
    zip.finish()?;
    Ok(bytes)
}
