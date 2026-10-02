#![allow(dead_code, non_snake_case, non_camel_case_types, unused_assignments, unused_variables, unused_mut, unused_imports, unused_must_use)]

#[path = "bin_core.rs"] pub mod bin;
#[path = "blacklist_core.rs"] pub mod blacklist;
#[path = "config_core.rs"] pub mod config;
#[path = "qc_core.rs"] pub mod qc;
#[path = "core_mod.rs"] pub mod core;
#[path = "custom_style_core.rs"] pub mod custom_style;
#[path = "dir_core.rs"] pub mod dir;
#[path = "ext_core.rs"] pub mod ext;
#[path = "fs_core.rs"] pub mod fs;
#[path = "info_core.rs"] pub mod info;
#[path = "judge_core.rs"] pub mod judge;
#[path = "parse_core.rs"] pub mod parse;
#[path = "particle_core.rs"] pub mod particle;
#[path = "scene_core.rs"] pub mod scene;
#[path = "task_core.rs"] pub mod task;
#[path = "time_core.rs"] pub mod time;
#[path = "ui_core.rs"] pub mod ui;
#[path = "moddir_core.rs"] pub mod moddir;
#[cfg(feature = "log")]
#[path = "log_core.rs"] pub mod log;

#[rustfmt::skip]
#[cfg(all(closed, not(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))))]
pub mod inner;

pub use scene::Main;

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Phira".to_string(),
        window_width: 973,
        window_height: 608,
        ..Default::default()
    }
}
