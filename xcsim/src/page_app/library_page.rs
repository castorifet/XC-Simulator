xcsim_core_l10n::tl_file!("library");

use super::{FavoritesPage, NextPage, Page, SharedState};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView, NEED_UPDATE},
    client::{recv_raw, Chart, ChartRef, Client, Collection, LocalCollection},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{favorites::FAV_PAGE_RESULT, ChartItem},
    popup::Popup,
    rate::RateDialog,
    save_data,
    scene::{check_read_tos_and_policy, compress_folder, confirm_dialog, ChartOrder, JUST_LOADED_TOS},
    tabs::{Tabs, TitleFn},
    tags::TagsDialog,
};
use anyhow::{anyhow, Error, Result};
use chrono::{DateTime, Utc};
use inputbox::InputBox;
#[cfg(target_os = "android")]
use jni::{jni_sig, jni_str, objects::JObject, refs::Global, vm::JavaVM, EnvUnowned};
use macroquad::prelude::*;
use xcsim_core::{
    ext::{poll_future, semi_black, semi_white, JoinToString, LocalTask, RectExt, SafeTexture},
    scene::{request_input, return_input, show_error, show_message, take_input, NextScene},
    task::Task,
    ui::{DRectButton, Dialog, RectButton, Ui},
};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufWriter, Write},
    mem,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
};
use tap::Tap;

pub static FAV_UPDATED: AtomicBool = AtomicBool::new(false);
pub static CHOOSE_COVER: AtomicBool = AtomicBool::new(false);

thread_local! {
    pub static CHOSEN_COVER: RefCell<Option<Result<i32, String>>> = const { RefCell::new(None) };
}

const PAGE_NUM: u64 = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartListType {
    Local,
    Ranked,
    Special,
    Unstable,
    Popular,
}

struct ChartList {
    ty: ChartListType,
    view: ChartsView,
}
impl ChartList {
    fn new(ty: ChartListType, icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let mut view = ChartsView::new(icons, rank_icons);
        view.can_refresh = ty != ChartListType::Local;
        Self { ty, view }
    }
}

struct CreateFavorite {
    name: String,
    charts: Vec<ChartRef>,
}

type OnlineTaskResult = (Vec<ChartDisplayItem>, Vec<Chart>, u64);
type OnlineTask = Task<Result<OnlineTaskResult>>;

pub struct LibraryPage {
    tabs: Tabs<ChartList>,

    current_page: u64,
    online_total_page: u64,
    prev_page_btn: DRectButton,
    next_page_btn: DRectButton,

    online_task: Option<OnlineTask>,

    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],
    current_folder: Option<String>,
    import_btn: DRectButton,

    search_btn: DRectButton,
    search_str: String,
    search_clr_btn: RectButton,

    order_btn: DRectButton,
    order_menu: Popup,
    order_menu_options: Vec<ChartOrder>,
    need_show_order_menu: bool,
    current_order: ChartOrder,
    order_meta_menu: Popup,
    need_show_order_meta_menu: bool,

    order_rev: bool,

    filter_btn: DRectButton,
    tags: TagsDialog,
    tags_last_show: bool,
    rating: RateDialog,
    rating_last_show: bool,
    filter_show_tag: bool,

    fav_btn: DRectButton,

    current_fav_index: Option<usize>,
    sync_fav_task: Option<Task<Result<Option<Collection>>>>,
    force_sync_to_cloud: Arc<AtomicBool>,

    multi_operation_btn: DRectButton,
    multi_operation_menu: Popup,
    multi_operation_options: Vec<&'static str>,
    need_show_multi_operation_menu: bool,

    multi_select_btn: DRectButton,
    multi_select_menu: Popup,
    need_show_multi_select_menu: bool,

    multi_select_cancel_btn: DRectButton,
    delete_multi: Arc<AtomicBool>,
    multi_create_fav_task: Option<Task<Result<CreateFavorite>>>,

    next_page: Option<NextPage>,
    next_page_task: LocalTask<Result<NextPage>>,
    pending_scene: Option<NextScene>,

    export_paths: Option<Vec<String>>,
    export_task: Option<mpsc::Receiver<Result<()>>>,
    export_progress: Arc<AtomicU32>,
    export_total: usize,

    xhus2_nav: [DRectButton; 5],
}

impl LibraryPage {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Result<Self> {
        NEED_UPDATE.store(true, Ordering::Relaxed);
        let _icon_star = icons.star.clone();
        let new_list = |ty| ChartList::new(ty, Arc::clone(&icons), rank_icons.clone());
        Ok(Self {
            tabs: Tabs::new([
                (new_list(ChartListType::Local), || tl!("local")),
                (new_list(ChartListType::Ranked), || ttl!("chart-ranked")),
                (new_list(ChartListType::Special), || ttl!("chart-special")),
                (new_list(ChartListType::Unstable), || ttl!("chart-unstable")),
                (new_list(ChartListType::Popular), || tl!("popular")),
            ] as [(ChartList, TitleFn); 5]),

            current_page: 0,
            online_total_page: 0,
            prev_page_btn: DRectButton::new(),
            next_page_btn: DRectButton::new(),

            online_task: None,

            icons,
            rank_icons,
            current_folder: None,
            import_btn: DRectButton::new(),

            search_btn: DRectButton::new(),
            search_str: String::new(),
            search_clr_btn: RectButton::new(),

            order_btn: DRectButton::new(),
            order_menu: Popup::new().with_size(0.5),
            order_menu_options: Vec::new(),
            need_show_order_menu: false,
            current_order: ChartOrder::Default,
            order_meta_menu: Popup::new().with_size(0.5),
            need_show_order_meta_menu: false,

            order_rev: true,

            filter_btn: DRectButton::new(),
            tags: TagsDialog::new(true).tap_mut(|it| it.perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default()),
            tags_last_show: false,
            rating: RateDialog::new(true).tap_mut(|it| {
                it.rate.score = 3;
                it.rate_upper.as_mut().unwrap().score = 10;
            }),
            rating_last_show: false,
            filter_show_tag: true,

            fav_btn: DRectButton::new(),
            current_fav_index: None,
            sync_fav_task: None,
            force_sync_to_cloud: Arc::default(),

            multi_operation_btn: DRectButton::new(),
            multi_operation_menu: Popup::new().with_size(0.5),
            multi_operation_options: Vec::new(),
            need_show_multi_operation_menu: false,

            multi_select_btn: DRectButton::new(),
            multi_select_menu: Popup::new()
                .with_size(0.5)
                .with_options(vec![tl!("multi-select-all").into_owned(), tl!("multi-select-invert").into_owned()]),
            need_show_multi_select_menu: false,

            multi_select_cancel_btn: DRectButton::new(),
            delete_multi: Arc::default(),
            multi_create_fav_task: None,

            next_page: None,
            next_page_task: None,
            pending_scene: None,

            export_paths: None,
            export_task: None,
            export_progress: Arc::default(),
            export_total: 0,

            xhus2_nav: std::array::from_fn(|_| DRectButton::new()),
        })
    }
}

impl LibraryPage {
    fn total_page(&self) -> u64 {
        if self.tabs.selected().ty == ChartListType::Local {
            0
        } else {
            self.online_total_page
        }
    }

    pub fn load_online(&mut self) {
        if get_data().config.offline_mode {
            show_message(tl!("offline-mode")).error();
            return;
        }
        if get_data().me.is_none() {
            show_error(anyhow!(tl!("must-login")));
            return;
        }
        if !check_read_tos_and_policy(false, false) {
            return;
        }
        self.tabs.selected_mut().view.reset_scroll();
        self.tabs.selected_mut().view.clear();
        let page = self.current_page;
        let search = self.search_str.clone();
        let order = {
            let order = match self.current_order {
                ChartOrder::Default => "updated",
                ChartOrder::Name => "name",
                ChartOrder::Rating => "rating",
                ChartOrder::Difficulty => "difficulty",
            };
            if self.order_rev {
                format!("-{order}")
            } else {
                order.to_owned()
            }
        };
        let tags = self
            .tags
            .tags
            .tags()
            .iter()
            .cloned()
            .chain(self.tags.unwanted.as_ref().unwrap().tags().iter().map(|it| format!("-{it}")))
            .join(",");
        let division = self.tags.division;
        let rating_range = format!("{},{}", self.rating.rate.score as f32 / 10., self.rating.rate_upper.as_ref().unwrap().score as f32 / 10.);
        let chosen = self.tabs.selected().ty;
        let popular = chosen == ChartListType::Popular;
        let typ = match chosen {
            ChartListType::Ranked => 0,
            ChartListType::Special => 1,
            ChartListType::Unstable => 2,
            _ => -1,
        };
        let by_me = if self.tags.show_me {
            get_data().me.as_ref().map(|it| it.id)
        } else {
            None
        };
        let show_unreviewed = self.tags.show_unreviewed;
        let show_stabilize = self.tags.show_stabilize;
        self.online_task = Some(Task::new(async move {
            let mut q = Client::query::<Chart>();
            if popular {
                q = q.suffix("/popular");
            } else {
                q = q.search(search).order(order).tags(tags).query("rating", rating_range);
            }
            if let Some(me) = by_me {
                q = q.query("uploader", me.to_string());
            }
            if show_stabilize {
                q = q.query("stableRequest", "true");
            } else if show_unreviewed {
                q = q.query("reviewed", "false").query("stableRequest", "false");
            }
            let (remote_charts, count) = q
                .query("type", typ.to_string())
                .query("division", division)
                .page(page)
                .page_num(PAGE_NUM)
                .send()
                .await?;
            let total_page = if count == 0 { 0 } else { (count - 1) / PAGE_NUM + 1 };
            let charts: Vec<_> = remote_charts.iter().map(ChartDisplayItem::from_remote).collect();
            Ok((charts, remote_charts, total_page))
        }));
    }

 fn sync_local(&mut self, s: &SharedState) {
    let list = self.tabs.selected_mut();
    if list.ty != ChartListType::Local {
        return;
    }

    let search = self.search_str.clone();
    let mut charts = Vec::new();

    let search_lc = search.to_lowercase();
    let matches_chart = |it: &ChartItem| -> bool {
        if search_lc.is_empty() {
            return true;
        }
        it.info.name.to_lowercase().contains(&search_lc)
    };

    if let Some(folder) = &self.current_folder {
        charts.push(ChartDisplayItem::new_back());

        charts.append(
            &mut s
                .charts_local
                .iter()
                .filter(|it| {
                    let Some(local_path) = &it.local_path else {
                        return false;
                    };
                    let first = local_path.split('/').next().unwrap_or("");
                    first == folder && matches_chart(it)
                })
                .map(|it| ChartDisplayItem::new(Some(it.clone()), None))
                .collect::<Vec<ChartDisplayItem>>(),
        );
    } else if !search.is_empty() {

        charts.push(ChartDisplayItem::new(None, None));
        charts.append(
            &mut s
                .charts_local
                .iter()
                .filter(|it| it.local_path.is_some() && matches_chart(it))
                .map(|it| ChartDisplayItem::new(Some(it.clone()), None))
                .collect::<Vec<ChartDisplayItem>>(),
        );
    } else {
        charts.push(ChartDisplayItem::new(None, None));

        use std::collections::BTreeSet;

        let folders: BTreeSet<String> = s
            .charts_local
            .iter()
            .filter_map(|it| {
                let local_path = it.local_path.as_ref()?;
                let first = local_path.split('/').next()?.to_string();
                if first.is_empty() {
                    None
                } else {
                    Some(first)
                }
            })
            .collect();

        for folder in folders {
            let title = match folder.as_str() {
    "@special" => "BM Default".to_string(),
    "special" => "XCBY\nChart\nCollection".to_string(),
    "download" => "Online".to_string(),
    "custom" => "Local".to_string(),
    _ => folder.clone(),
};

charts.push(ChartDisplayItem::new_folder(folder, title));
        }
    }

    list.view.set(s.t, charts);
}

    fn on_order_update(&mut self, s: &mut SharedState) {
        let list = self.tabs.selected_mut();
        if list.ty == ChartListType::Local {
            self.sync_local(s);
        } else {
            self.current_page = 0;
            self.load_online();
        }
    }

    fn check_fav_page(&mut self, s: &mut SharedState) {
        if let Some(result) = FAV_PAGE_RESULT.with(|it| it.borrow_mut().take()) {
            self.current_fav_index = result;
            self.sync_local(s);
        }
    }

    fn update_order_meta_menu_options(&mut self) {
        self.order_meta_menu.set_options(vec![
            tl!("order-by", "order" => self.current_order.label()),
            if self.order_rev { tl!("order-desc") } else { tl!("order-asc") }.into(),
        ]);
    }
}

struct ExportConfig {
    file: File,
    deleter: Box<dyn FnOnce() -> io::Result<()> + Send>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportInfo {
    pub exported_at: DateTime<Utc>,
    pub version: String,
}

static EXPORT_CONFIG: Mutex<Option<io::Result<ExportConfig>>> = Mutex::new(None);
#[cfg(target_os = "ios")]
static EXPORT_PICKER_PATH: Mutex<Option<String>> = Mutex::new(None);

#[cfg(target_os = "ios")]
fn present_export_picker(path: String) {
    use objc2::{available, define_class, rc::Retained, runtime::ProtocolObject, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
    use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};

    thread_local! {
        static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
    }

    define_class! {

        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct PickerDelegate;

        unsafe impl NSObjectProtocol for PickerDelegate {}

        unsafe impl UIDocumentPickerDelegate for PickerDelegate {

            #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
            fn did_pick_documents_at_urls(&self, _controller: &UIDocumentPickerViewController, _urls: &NSArray<NSURL>) {
                show_message(tl!("multi-exported")).ok();
            }
        }
    }

    impl PickerDelegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { objc2::msg_send![super(this), init] }
        }
    }

    let mtm = MainThreadMarker::new().unwrap();

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
    let urls = NSArray::from_retained_slice(&[url]);
    let picker = UIDocumentPickerViewController::alloc(mtm);
    let picker = if available!(ios = 14.0.0) {
        UIDocumentPickerViewController::initForExportingURLs_asCopy(picker, &urls, true)
    } else {
        #[allow(deprecated)]
        {
            use objc2_ui_kit::UIDocumentPickerMode;
            UIDocumentPickerViewController::initWithURLs_inMode(picker, &urls, UIDocumentPickerMode::ExportToService)
        }
    };
    let dlg_obj = PickerDelegate::new(mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*dlg_obj)));
    DELEGATE.with(|it| *it.borrow_mut() = Some(dlg_obj));

    if let Some(controller) = inputbox::backend::IOS::get_top_view_controller(mtm) {
        controller.presentViewController_animated_completion(&picker, true, None);
    } else {
        show_error(Error::msg("Failed to present export dialog"));
    }
}

fn request_export() {
    let suggested_name = format!("phira-export-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    cfg_if::cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                let method =
                    (**env).GetMethodID.unwrap()(env, class, c"showExportDialog".as_ptr() as _, c"(Ljava/lang/String;)V".as_ptr() as _);
                let url = std::ffi::CString::new(suggested_name).unwrap();
                (**env).CallVoidMethod.unwrap()(
                    env,
                    ctx,
                    method,
                    (**env).NewStringUTF.unwrap()(env, url.as_ptr()),
                );
            }
        } else if #[cfg(target_os = "ios")] {
            use objc2_foundation::NSTemporaryDirectory;

            let dir = NSTemporaryDirectory();
            let output_path = PathBuf::from(dir.to_string()).join(&suggested_name);
            let output_path_str = output_path.to_string_lossy().to_string();
            let config = File::create(&output_path).map(|file| {
                let delete_path = output_path.clone();
                ExportConfig {
                    file,
                    deleter: Box::new(move || std::fs::remove_file(delete_path)),
                }
            });
            if config.is_ok() {
                EXPORT_PICKER_PATH.lock().unwrap().replace(output_path_str);
            }
            EXPORT_CONFIG.lock().unwrap().replace(config);
        } else if #[cfg(target_env = "ohos")] {
            miniquad::native::call_request_callback(format!("{{\"action\":\"request_export\",\"filename\":\"{}\"}}", suggested_name));
        } else {
            if let Some(output_path) = rfd::FileDialog::new().set_title(tl!("multi-export-title")).set_file_name(&suggested_name).save_file() {
                let config = File::create(&output_path).map(|file| ExportConfig {
                    file,
                    deleter: Box::new(move || std::fs::remove_file(output_path)),
                });
                EXPORT_CONFIG.lock().unwrap().replace(config);
            }
        }
    }
}

#[cfg(target_os = "android")]
fn delete_uri(uri: Global<JObject<'static>>) {
    JavaVM::singleton()
        .unwrap()
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let ctx = ndk_context::android_context().context();
            let ctx = unsafe { JObject::from_raw(env, ctx as _) };
            env.call_method(ctx, jni_str!("deleteUri"), jni_sig!("(Landroid/net/Uri;)V"), &[uri.as_ref().into()])?;
            Ok(())
        })
        .unwrap();
}

#[cfg(target_os = "android")]
#[export_name = "Java_quad_1native_QuadNative_processExportFd"]
extern "system" fn process_export_fd(mut env: EnvUnowned, _: jni::objects::JClass, uri: jni::objects::JObject, fd: jni::sys::jint) {
    use std::os::fd::FromRawFd;
    env.with_env(|env| -> jni::errors::Result<()> {
        let uri = env.new_global_ref(uri)?;
        let file = unsafe { File::from_raw_fd(fd as _) };
        EXPORT_CONFIG.lock().unwrap().replace(Ok(ExportConfig {
            file,
            deleter: Box::new(|| {
                delete_uri(uri);
                Ok(())
            }),
        }));
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[cfg(target_env = "ohos")]
mod ohos_export {
    use super::*;
    use napi_derive_ohos::napi;
    #[napi]
    #[allow(dead_code)]
    pub fn process_export_fd_ohos(fd: u32) {
        use std::os::fd::FromRawFd;
        let file = unsafe { File::from_raw_fd(fd as _) };
        EXPORT_CONFIG.lock().unwrap().replace(Ok(ExportConfig {
            file,
            deleter: Box::new(|| Ok(())),
        }));
    }
}

impl LibraryPage {

    fn touch_xhus2(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        let rt = s.rt;

        if self.sync_fav_task.is_some() || self.export_task.is_some() || self.multi_create_fav_task.is_some() {
            return Ok(true);
        }

        if self.order_menu.showing() { self.order_menu.touch(touch, t); return Ok(true); }
        if self.order_meta_menu.showing() { self.order_meta_menu.touch(touch, t); return Ok(true); }
        if self.multi_operation_menu.showing() { self.multi_operation_menu.touch(touch, t); return Ok(true); }
        if self.multi_select_menu.showing() { self.multi_select_menu.touch(touch, t); return Ok(true); }

        if self.tags.touch(touch, t) { return Ok(true); }
        if self.rating.touch(touch, t) { return Ok(true); }
        if self.tags.showing() || self.rating.showing() { return Ok(true); }

        for i in 0..5 {
            if self.xhus2_nav[i].touch(touch, rt) {
                self.tabs.goto(rt, i);
                self.current_page = 0;
                self.tabs.selected_mut().view.reset_scroll();
                return Ok(true);
            }
        }

        let chosen = self.tabs.selected().ty;
        if chosen != ChartListType::Local {
            let total_page = self.total_page();
            if self.prev_page_btn.touch(touch, rt) && self.current_page > 0 {
                self.current_page -= 1;
                self.online_task = None;
                return Ok(true);
            }
            if self.next_page_btn.touch(touch, rt) && self.current_page + 1 < total_page {
                self.current_page += 1;
                self.online_task = None;
                return Ok(true);
            }
        }

        if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
            self.search_str.clear();
            self.tabs.selected_mut().view.reset_scroll();
            return Ok(true);
        }

        if self.order_btn.touch(touch, rt) { self.need_show_order_menu = true; return Ok(true); }
        if chosen == ChartListType::Local {
            if self.import_btn.touch(touch, rt) {
                xcsim_core::scene::request_file("_import_auto");
                return Ok(true);
            }
        } else if self.filter_btn.touch(touch, rt) {
            if self.filter_show_tag {
                if self.tags.showing() { self.tags.dismiss(rt); } else { self.tags.enter(rt); }
            } else {
                if self.rating.showing() { self.rating.dismiss(rt); } else { self.rating.enter(rt); }
            }
            return Ok(true);
        }
        if self.search_btn.touch(touch, rt) {
            xcsim_core::scene::request_input("search", inputbox::InputBox::new().default_text(&self.search_str));
            return Ok(true);
        }

        if self.tabs.selected_mut().view.touch(touch, t, rt)? {
            return Ok(true);
        }

        Ok(false)
    }

    fn render_xhus2(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;
        let top = ui.top;
        let bar_y = -top;
        let bar_h = 0.155_f32;
        let margin = 0.045_f32;
        let full_x = -1.0 + margin;
        let full_w = (1.0 - margin) - full_x;
        let body_y = bar_y + bar_h;
        let tab_h = 0.105_f32;
        let tab_gap = 0.018_f32;
        let content_y = body_y + tab_h + 0.026;
        let content_bottom = top - margin;
        let accent = crate::theme::FIREFLY_PINK_DEEP;
        let accent_soft = crate::theme::FIREFLY_PINK;
        let cream = crate::theme::FIREFLY_CREAM_SOFT;
        let sidebar_bg = Color::new(0.215, 0.094, 0.157, 0.98);
        let dark_bg = Color::new(0.243, 0.110, 0.176, 0.98);
        let backdrop = Color::new(0.165, 0.067, 0.118, 1.0);

        s.fader.render(ui, t, |ui| {
            let chosen = self.tabs.selected().ty;

            ui.fill_rect(Rect::new(-1., bar_y, 2., top * 2.), backdrop);

            ui.fill_rect(Rect::new(-1., bar_y, 2., bar_h), Color::new(1.0, 0.58, 0.706, 0.06));
            for i in 0..8 {
                ui.text("♡")
                    .pos(0.30 + i as f32 * 0.105, bar_y + 0.028)
                    .anchor(0.5, 0.5).no_baseline().size(0.28)
                    .color(Color::new(1.0, 0.776, 0.847, 0.10)).draw();
            }

            let br = ui.back_rect();
            ui.fill_path(&br.feather(-0.004).rounded(br.h * 0.5), Color::new(1.0, 0.58, 0.706, 0.16));
            ui.text("←")
                .pos(br.center().x, br.center().y)
                .anchor(0.5, 0.5).no_baseline().size(0.5)
                .color(accent).draw();

            let title_x = br.right() + 0.04;
            let title_r = ui.text("Library")
                .pos(title_x, bar_y + bar_h * 0.42)
                .anchor(0., 0.5).no_baseline().size(0.82)
                .color(cream).draw();
            ui.text("♥")
                .pos(title_r.right() + 0.028, title_r.center().y)
                .anchor(0., 0.5).no_baseline().size(0.5)
                .color(accent).draw();
            ui.fill_path(&Rect::new(title_x, bar_y + bar_h - 0.022, title_r.w.min(0.22), 0.006).rounded(0.003), accent_soft);

            let btn_h = bar_h - 0.04;
            let btn_y = bar_y + 0.02;
            let mut bx = 0.95_f32;

            let bw = 0.14_f32;
            bx -= bw;
            let r = Rect::new(bx, btn_y, bw - 0.01, btn_h);
            self.order_btn.render_shadow(ui, r, rt, |ui, path| {
                ui.fill_path(&path, dark_bg);
                ui.text("Order").pos(r.center().x, r.center().y).anchor(0.5, 0.5).no_baseline().size(0.40).color(WHITE).draw();
            });
            if self.need_show_order_menu {
                self.need_show_order_menu = false;
                if chosen == ChartListType::Local {
                    self.order_menu_options = vec![ChartOrder::Default, ChartOrder::Name, ChartOrder::Difficulty];
                } else {
                    self.order_menu_options = vec![ChartOrder::Default, ChartOrder::Rating, ChartOrder::Name, ChartOrder::Difficulty];
                }
                self.order_menu.set_selected(
                    self.order_menu_options.iter().position(|&it| it == self.current_order).unwrap_or(usize::MAX),
                );
                self.order_menu.set_options(self.order_menu_options.iter().map(|it| it.label().into_owned()).collect());
                self.order_menu.show(ui, t, Rect::new(bx - 0.22, bar_y + bar_h + 0.01, 0.30, 0.45));
            }
            bx -= 0.01;

            let bw = 0.16_f32;
            bx -= bw;
            let r = Rect::new(bx, btn_y, bw - 0.01, btn_h);
            let search_label = if self.search_str.is_empty() { "Search" } else { &self.search_str };
            self.search_btn.render_shadow(ui, r, rt, |ui, path| {
                ui.fill_path(&path, if self.search_str.is_empty() { dark_bg } else { accent });
                ui.text(search_label).pos(r.center().x, r.center().y).anchor(0.5, 0.5).no_baseline().size(0.36).max_width(bw - 0.02).color(WHITE).draw();
            });
            if !self.search_str.is_empty() {
                let clr_r = Rect::new(r.right() - 0.05, r.y + 0.01, 0.04, r.h - 0.02);
                self.search_clr_btn.set(ui, clr_r);
                ui.text("✕").pos(clr_r.center().x, clr_r.center().y).anchor(0.5, 0.5).no_baseline().size(0.32).color(semi_white(0.7)).draw();
            }
            bx -= 0.01;

            let bw = 0.16_f32;
            bx -= bw;
            let r = Rect::new(bx, btn_y, bw - 0.01, btn_h);
            if chosen == ChartListType::Local {
                self.import_btn.render_shadow(ui, r, rt, |ui, path| {
                    ui.fill_path(&path, dark_bg);
                    ui.text("Import").pos(r.center().x, r.center().y).anchor(0.5, 0.5).no_baseline().size(0.40).color(WHITE).draw();
                });
            } else {
                self.filter_btn.render_shadow(ui, r, rt, |ui, path| {
                    let active = self.tags.showing() || self.rating.showing();
                    ui.fill_path(&path, if active { accent } else { dark_bg });
                    ui.text("Filter").pos(r.center().x, r.center().y).anchor(0.5, 0.5).no_baseline().size(0.40).color(WHITE).draw();
                });
            }

            if self.need_show_order_meta_menu {
                self.need_show_order_meta_menu = false;
                self.order_meta_menu.show(ui, rt, Rect::new(bx, bar_y + bar_h + 0.02, 0.35, 0.5));
            }
            if self.need_show_multi_operation_menu {
                self.need_show_multi_operation_menu = false;
                self.multi_operation_menu.show(ui, rt, Rect::new(-0.3, bar_y + bar_h + 0.02, 0.35, 0.5));
            }

            let sel = self.tabs.selected_idx();
            let pill_w = (full_w - tab_gap * 4.) / 5.;
            for i in 0..5 {
                let tx = full_x + i as f32 * (pill_w + tab_gap);
                let tr = Rect::new(tx, body_y, pill_w, tab_h);
                let active = i == sel;
                if active {
                    ui.fill_path(&tr.feather(0.006).rounded(tab_h * 0.5), Color::new(1.0, 0.58, 0.706, 0.25));
                    ui.fill_path(&tr.rounded(tab_h * 0.5), accent);
                } else {
                    ui.fill_path(&tr.rounded(tab_h * 0.5), sidebar_bg);
                    ui.stroke_path(&tr.rounded(tab_h * 0.5), 0.0025, Color::new(1.0, 0.776, 0.847, 0.18));
                }
                let label = self.tabs.title(i);
                ui.text(format!("{} {}", if active { "♥" } else { "♡" }, label))
                    .pos(tr.center().x, tr.center().y)
                    .anchor(0.5, 0.5).no_baseline().size(0.44)
                    .max_width(pill_w - 0.03)
                    .color(if active { WHITE } else { semi_white(0.7) })
                    .draw();
                self.xhus2_nav[i].render_shadow(ui, tr, rt, |_, _| {});
            }

            let pag_h = 0.085_f32;
            let pag_reserve = if chosen != ChartListType::Local { pag_h + 0.024 } else { 0.0 };
            let content_h = content_bottom - content_y - pag_reserve;

            let content_card = Rect::new(full_x, content_y, full_w, content_h);
            ui.fill_path(&content_card.feather(0.01).rounded(0.04), Color::new(1.0, 0.58, 0.706, 0.16));
            ui.fill_path(&content_card.rounded(0.035), dark_bg);
            let content_inner = Rect::new(full_x + 0.016, content_y + 0.016, full_w - 0.032, content_h - 0.032);

            if chosen != ChartListType::Local {
                let total_page = self.total_page();
                let pag_w = 0.46_f32;
                let pag_x = -pag_w / 2.;
                let pag_y = content_y + content_h + 0.012;
                let pag_row = Rect::new(pag_x, pag_y, pag_w, pag_h);
                ui.fill_path(&pag_row.rounded(pag_h * 0.5), Color::new(1.0, 0.58, 0.706, 0.10));
                let side_w = 0.09_f32;
                let prev_r = Rect::new(pag_x + 0.014, pag_y + 0.012, side_w, pag_h - 0.024);
                let next_r = Rect::new(pag_row.right() - side_w - 0.014, pag_y + 0.012, side_w, pag_h - 0.024);

                self.prev_page_btn.render_shadow(ui, prev_r, rt, |ui, path| {
                    ui.fill_path(&path, if self.current_page > 0 { accent } else { semi_black(0.4) });
                    ui.text("◀").pos(prev_r.center().x, prev_r.center().y).anchor(0.5, 0.5).no_baseline().size(0.46).color(WHITE).draw();
                });

                ui.text(format!("{} / {}", self.current_page + 1, total_page.max(1)))
                    .pos(0., pag_y + pag_h * 0.5)
                    .anchor(0.5, 0.5).no_baseline().size(0.46).color(cream).draw();

                self.next_page_btn.render_shadow(ui, next_r, rt, |ui, path| {
                    ui.fill_path(&path, if self.current_page + 1 < total_page { accent } else { semi_black(0.4) });
                    ui.text("▶").pos(next_r.center().x, next_r.center().y).anchor(0.5, 0.5).no_baseline().size(0.46).color(WHITE).draw();
                });
            }

            let chart_list = self.tabs.selected_mut();
            ui.scissor(content_inner, |ui| {
                chart_list.view.render_xhus2(ui, content_inner, t);
            });

            self.tags.render(ui, rt);
            self.rating.render(ui, rt);

            self.order_menu.render(ui, t, 1.);
            self.order_meta_menu.render(ui, t, 1.);
            self.multi_operation_menu.render(ui, t, 1.);
            self.multi_select_menu.render(ui, t, 1.);

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}

impl Page for LibraryPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn custom_title(&self) -> bool {
        true
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if FAV_UPDATED.swap(false, Ordering::SeqCst) {
            self.sync_local(s);
        }
        Ok(())
    }

    fn on_result(&mut self, res: Box<dyn Any>, s: &mut SharedState) -> Result<()> {
        let _res = match res.downcast::<bool>() {
            Err(res) => res,
            Ok(delete) => {
                self.tabs.selected_mut().view.on_result(s.t, *delete);
                return Ok(());
            }
        };
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        self.touch_xhus2(touch, s)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        if let Some(_chosen_cover) = CHOSEN_COVER.with(|it| it.borrow_mut().take()) {
                Dialog::simple("Favorites is not avaliable").show();
        }

        self.check_fav_page(s);

        if self.tabs.selected().ty == ChartListType::Local && self.current_order == ChartOrder::Rating {
            self.current_order = ChartOrder::Default;
            self.order_rev = true;
        }

        self.tags.update(t);
        self.rating.update(t);

        let is_local = self.tabs.selected().ty == ChartListType::Local;
        if self.tabs.changed() {
            self.tabs.selected_mut().view.reset_scroll();
            self.tabs.iter_mut().for_each(|it| it.view.multi_select = None);
            self.online_task = None;
            if is_local {
                self.sync_local(s);
            } else {
                self.current_page = 0;
                self.load_online();
            }
        }

        if let Some(task) = &mut self.next_page_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.next_page = Some(res?);
                self.next_page_task = None;
            }
        }

        if self.tags.show_rating {
            self.tags.show_rating = false;
            self.filter_show_tag = false;
            self.rating.enter(t);
        } else if self.tags_last_show && !self.tags.showing() {
            self.current_page = 0;
            self.load_online();
        }
        if self.rating.show_tags {
            self.rating.show_tags = false;
            self.filter_show_tag = true;
            self.tags.enter(t);
        } else if self.rating_last_show && !self.rating.showing() {
            self.current_page = 0;
            self.load_online();
        }
        self.tags_last_show = self.tags.showing();
        self.rating_last_show = self.rating.showing();
        if let Some(task) = &mut self.online_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("failed-to-load-online"))),
                    Ok(res) => {
                        self.online_total_page = res.2;
                        self.tabs.selected_mut().view.set(t, res.0);
                    }
                }
                self.online_task = None;
            }
        }
        self.order_menu.update(t);
        self.order_meta_menu.update(t);
        self.multi_operation_menu.update(t);
        self.multi_select_menu.update(t);
        for chart in &mut s.charts_local {
            chart.illu.settle(t);
        }
        if let Some(folder) = self.tabs.selected_mut().view.clicked_folder.take() {
    self.current_folder = Some(folder);
    self.tabs.selected_mut().view.reset_scroll();
    self.sync_local(s);
}

if self.tabs.selected_mut().view.clicked_back {
    self.tabs.selected_mut().view.clicked_back = false;
    self.current_folder = None;
    self.tabs.selected_mut().view.reset_scroll();
    self.sync_local(s);
}

if self.tabs.selected_mut().view.clicked_special {
    self.tabs.selected_mut().view.clicked_special = false;
    let scene = crate::scene::ChaptersScene::new(Arc::clone(&self.icons), self.rank_icons.clone());
    self.pending_scene = Some(NextScene::Overlay(Box::new(scene)));
}
        if self.tabs.selected_mut().view.update(t)? {
            self.load_online();
        }
        if self.tabs.selected_mut().view.need_update() {
            s.reload_local_charts();
            self.sync_local(s);
        }
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                if is_local {
                    self.sync_local(s);
                } else {
                    self.current_page = 0;
                    self.load_online();
                }
            } else if id == "new_fav" {
                if text.is_empty() {
                    use crate::page::favorites::{tl as ftl, L10N_LOCAL};
                    show_message(ftl!("name-empty")).error();
                } else {
                    let charts_view = &mut self.tabs.selected_mut().view;
                    if let Some(mut selected) = charts_view.multi_select.clone() {
                        self.multi_create_fav_task = Some(Task::new(async move {
                            let mut ids_str = String::new();
                            for chart in &selected {
                                if let ChartRef::Online(id, None) = chart {
                                    ids_str.push_str(&id.to_string());
                                    ids_str.push(',');
                                }
                            }
                            if !ids_str.is_empty() {
                                ids_str.pop();
                                let resp: Vec<Chart> = recv_raw(Client::get(format!("/chart/multi-get?ids={ids_str}"))).await?.json().await?;
                                let mut id_to_chart = HashMap::new();
                                for chart in resp {
                                    id_to_chart.insert(chart.id, Box::new(chart));
                                }
                                for chart in &mut selected {
                                    if let ChartRef::Online(id, chart_info) = chart {
                                        if chart_info.is_none() {
                                            *chart_info = Some(id_to_chart.get(id).cloned().unwrap());
                                        }
                                    }
                                }
                            }
                            Ok(CreateFavorite {
                                name: text,
                                charts: selected,
                            })
                        }));
                    }
                }
            } else {
                return_input(id, text);
            }
        }
        if let Some(task) = &mut self.multi_create_fav_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err),
                    Ok(result) => {
                        self.tabs.selected_mut().view.multi_select = None;
                        let data = get_data_mut();
                        let mut col = LocalCollection::new(result.name);
                        col.charts = result.charts;
                        data.push_collection(col)?;
                        let _ = save_data();
                        show_message(tl!("fav-created")).ok();
                        self.current_fav_index = Some(data.collection_uuids().len() - 1);
                        self.sync_local(s);
                    }
                }
                self.multi_create_fav_task = None;
            }
        }
        if self.delete_multi.swap(false, Ordering::Relaxed) {
            let selected = self.tabs.selected_mut().view.multi_select.take().unwrap();
            let selected = selected.into_iter().collect::<HashSet<_>>();
            let data = get_data_mut();
            let mut local_paths = HashSet::new();
            for chart in &selected {
                let path = chart.local_path();
                match std::fs::remove_dir_all(format!("{}/{path}", dir::charts()?)) {
                    Ok(_) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
                local_paths.insert(path);
            }
            data.charts.retain(|it| !local_paths.contains(it.local_path.as_str()));
            let _ = save_data();
            show_message(tl!("multi-deleted")).ok();
            s.reload_local_charts();
            self.sync_local(s);
        }
        if self.order_meta_menu.changed() {
            match self.order_meta_menu.selected() {
                0 => {
                    self.need_show_order_menu = true;
                }
                1 => {
                    self.order_rev = !self.order_rev;
                    self.update_order_meta_menu_options();
                    self.order_meta_menu.set_selected(usize::MAX);
                    self.on_order_update(s);
                }
                _ => {}
            }
        }
        if self.order_menu.changed() {
            self.current_order = self.order_menu_options[self.order_menu.selected()];
            self.order_rev = matches!(self.current_order, ChartOrder::Default | ChartOrder::Rating);
            self.order_meta_menu.set_selected(usize::MAX);
            self.update_order_meta_menu_options();
            self.on_order_update(s);
        }
        if self.multi_operation_menu.changed() {
            let charts_view = &mut self.tabs.selected_mut().view;
            let selected = charts_view.multi_select.as_mut().unwrap();
            match self.multi_operation_options[self.multi_operation_menu.selected()] {
                "multi-export" => {
                    let charts = dir::charts()?;
                    let mut paths = Vec::with_capacity(selected.len());
                    let mut non_existent = Vec::new();
                    for chart in selected {
                        let path: PathBuf = format!("{charts}/{}", chart.local_path()).into();
                        if !path.exists() {
                            let mut charts = charts_view.charts.as_ref().unwrap().iter().filter_map(|it| it.chart.as_ref());
                            non_existent.push(charts.find(|it| &it.to_ref() == chart).unwrap().info.name.clone());
                        } else {
                            paths.push(chart.local_path().into_owned());
                        }
                    }
                    if !non_existent.is_empty() {
                        Dialog::simple(tl!("multi-export-no-file", "charts" => non_existent.join(", "))).show();
                    } else {
                        self.export_paths = Some(paths);
                        request_export();
                    }
                }
                "multi-create-fav" => {
                    request_input("new_fav", InputBox::new());
                }
                "multi-delete" => {
                    confirm_dialog(ttl!("del-confirm"), tl!("multi-delete-confirm", "count" => selected.len()), self.delete_multi.clone());
                }
                _ => {}
            }
        }
        if self.multi_select_menu.changed() {
            let charts_view = &mut self.tabs.selected_mut().view;
            let sel = charts_view.multi_select.as_mut().unwrap();
            let charts = charts_view.charts.as_ref().unwrap();
            match self.multi_select_menu.selected() {
                0 => {
                    sel.clear();
                    sel.extend(charts.iter().filter_map(|it| it.chart.as_ref()).map(ChartItem::to_ref));
                }
                1 => {
                    let old_sel = mem::take(sel).into_iter().collect::<HashSet<_>>();
                    for chart in charts {
                        if let Some(chart) = &chart.chart {
                            let r = chart.to_ref();
                            if !old_sel.contains(&r) {
                                sel.push(r);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(false, false);
        }
        let list = self.tabs.selected_mut();
        let view = &mut list.view;
        if let Some((from, to)) = view.take_movement() {
            if self.current_order != ChartOrder::Default && self.current_fav_index.is_none() {
                show_message(tl!("order-update-failed-sort")).error();
                return Ok(());
            }
            let data = get_data_mut();
            if let Some(index) = self.current_fav_index {
                let uuid = data.collection_uuids()[index];
                let mut col = data.collection_info(&uuid).as_ref().clone();
                let online = col.id.is_some();
                let chart = col.charts.remove(from);
                col.charts.insert(to, chart);
                data.set_collection_info(&uuid, col)?;
                let _ = save_data();
                if online && !data.config.offline_mode {
                    if let Some(task) = FavoritesPage::sync_to_cloud_task(index, false) {
                        self.sync_fav_task = Some(task);
                    }
                }
            } else {
                if self.order_rev {
                    let chart = data.charts.remove(data.charts.len() - from - 1);
                    data.charts.insert(data.charts.len() - to, chart);
                } else {
                    let chart = data.charts.remove(from);
                    data.charts.insert(to, chart);
                }
                let _ = save_data();
                s.reload_local_charts();
            }
            show_message(tl!("order-updated")).ok();
        }
        view.allow_edit(
            list.ty == ChartListType::Local
                && self.search_str.is_empty()
                && self.current_fav_index.is_none_or(|it| get_data().collection_by_index(it).is_owned()),
        );

        if let Some(task) = &mut self.sync_fav_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("fav-sync-failed"))),
                    Ok(Some(col)) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.current_fav_index.unwrap()];
                        let local = data.collection_info(&uuid);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        let _ = save_data();
                        show_message(tl!("fav-synced")).ok();
                    }
                    Ok(None) => {
                        use crate::page::favorites::{tl as ftl, L10N_LOCAL};
                        confirm_dialog(ftl!("sync-to-cloud"), ftl!("sync-outdated"), self.force_sync_to_cloud.clone());
                    }
                }
                self.sync_fav_task = None;
            }
        }
        if self.force_sync_to_cloud.swap(false, Ordering::SeqCst) {
            if let Some(index) = self.current_fav_index {
                if let Some(task) = FavoritesPage::sync_to_cloud_task(index, true) {
                    self.sync_fav_task = Some(task);
                }
            }
        }
        if let Some(config) = EXPORT_CONFIG.lock().unwrap().take() {
            fn export_inner(paths: Vec<String>, output: File, progress: Arc<AtomicU32>) -> Result<()> {
                let charts = dir::charts()?;
                let mut zip = zip::ZipWriter::new(BufWriter::new(output));
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o755);
                for (i, name) in paths.iter().enumerate() {
                    zip.start_file(format!("{name}.zip"), options)?;
                    let chart_bytes = compress_folder(Path::new(&format!("{charts}/{name}")))?;
                    zip.write_all(&chart_bytes)?;
                    progress.store(i as u32 + 1, Ordering::Relaxed);
                }

                zip.start_file("export.json", options.compression_method(zip::CompressionMethod::Deflated))?;
                let info = ExportInfo {
                    exported_at: Utc::now(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                };
                serde_json::to_writer(&mut zip, &info)?;

                zip.finish()?;
                Ok(())
            }

            match config {
                Err(err) => show_error(err.into()),
                Ok(config) => {
                    if let Some(paths) = self.export_paths.take() {
                        self.export_total = paths.len();
                        let (tx, rx) = mpsc::sync_channel(1);
                        let progress = self.export_progress.clone();
                        progress.store(0, Ordering::SeqCst);
                        std::thread::spawn(move || {
                            let result = export_inner(paths, config.file, progress);
                            if result.is_err() {
                                if let Err(err) = (config.deleter)() {
                                    warn!("failed to delete export file: {:?}", err);
                                }
                            }
                            let _ = tx.send(result);
                        });
                        self.export_task = Some(rx);
                    }
                }
            }
        }
        if let Some(rx) = &mut self.export_task {
            match rx.try_recv() {
                Ok(Err(err)) => {
                    show_error(err);
                    self.export_task = None;
                }
                Ok(Ok(())) => {
                    #[cfg(target_os = "ios")]
                    {
                        if let Some(path) = EXPORT_PICKER_PATH.lock().unwrap().clone() {
                            present_export_picker(path);
                        } else {
                            show_message(tl!("multi-exported")).ok();
                        }
                    }
                    #[cfg(not(target_os = "ios"))]
                    show_message(tl!("multi-exported")).ok();
                    self.tabs.selected_mut().view.multi_select = None;
                    self.export_task = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    show_error(Error::msg("Export thread panicked"));
                    self.export_task = None;
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        self.check_fav_page(s);
        self.render_xhus2(ui, s)
    }

    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.tabs.selected_mut().view.render_top(ui, t);
        if self.sync_fav_task.is_some() {
            ui.full_loading_simple(t);
        }
        if self.export_task.is_some() {
            let current = self.export_progress.load(Ordering::Relaxed);
            let total = self.export_total;
            ui.full_loading(tl!("multi-exporting", "current" => current, "total" => total), t);
        }
        if self.multi_create_fav_task.is_some() {
            ui.full_loading_simple(t);
        }
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        if let Some(scene) = self.pending_scene.take() {
            return scene;
        }
        self.tabs.selected_mut().view.next_scene().unwrap_or_default()
    }
}
