use crate::{anim::Anim, get_data, Result};
use macroquad::prelude::*;
use xcsim_core::{
    ext::{semi_black, RectExt},
    ui::{button_hit, rounded_rect_shadow, RectButton, ShadowConfig, Ui},
};
use std::borrow::Cow;

pub type TitleFn = fn() -> Cow<'static, str>;

struct TabItem<T> {
    value: T,
    title: TitleFn,
    btn: RectButton,
}

pub struct Tabs<T> {
    items: Vec<TabItem<T>>,
    selected: usize,

    x_left: Anim<f32>,
    x_right: Anim<f32>,

    content_progress: Anim<f32>,
    prev_go_up: bool,
    prev: usize,

    changed: bool,
}

impl<T> Tabs<T> {
    pub const TAB_HEIGHT: f32 = 0.15;
    const DURATIONS: (f32, f32) = (0.24, 0.35);
    const CONTENT_DY: f32 = 0.06;
    const CONTENT_DURATION: f32 = 0.4;

    pub fn new(items: impl IntoIterator<Item = (T, TitleFn)>) -> Self {
        Tabs {
            items: items
                .into_iter()
                .map(|(value, title)| TabItem {
                    value,
                    title,
                    btn: RectButton::new(),
                })
                .collect(),
            selected: 0,

            x_left: Anim::new(0.),
            x_right: Anim::new(0.),

            content_progress: Anim::new(1.),
            prev_go_up: false,
            prev: 0,

            changed: false,
        }
    }

    pub fn selected(&self) -> &T {
        &self.items[self.selected].value
    }

    pub fn selected_mut(&mut self) -> &mut T {
        &mut self.items[self.selected].value
    }

    pub fn selected_idx(&self) -> usize {
        self.selected
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn title(&self, idx: usize) -> Cow<'static, str> {
        (self.items[idx].title)()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut().map(|item| &mut item.value)
    }

    pub fn changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn goto(&mut self, t: f32, index: usize) {
        if index == self.selected {
            return;
        }

        let (mut upper, mut lower) = if get_data().prefer_reduced_motion { (0., 0.) } else { Self::DURATIONS };
        if index > self.selected {
            std::mem::swap(&mut upper, &mut lower);
            self.prev_go_up = true;
        } else {
            self.prev_go_up = false;
        }

        self.prev = self.selected;
        self.selected = index;
        self.x_left.begin(t, upper);
        self.x_right.begin(t, lower);
        self.content_progress.start(0., 1., t, Self::CONTENT_DURATION);

        self.changed = true;
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.btn.touch(touch) {
                button_hit();
                self.goto(t, index);
                return true;
            }
        }

        false
    }

    fn render_plain(&mut self, ui: &mut Ui, c: Color, first: bool, cr: Rect) {
        let n = self.items.len();
        let tab_w = cr.w / n as f32;
        let tab_y = cr.bottom() - Self::TAB_HEIGHT;
        let mut r = Rect::new(cr.x, tab_y, tab_w, Self::TAB_HEIGHT);

        for (index, item) in self.items.iter_mut().enumerate() {
            if index == self.selected {
                self.x_left.alter_to(r.x);
                self.x_right.alter_to(r.right());
            }
            item.btn.set(ui, r);
            if first {
                ui.fill_rect(r, semi_black(0.4 * c.a));
            }
            ui.text((item.title)())
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.42)
                .color(c)
                .draw();
            r.x += tab_w;
        }
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, cr: Rect, mut f: impl FnMut(&mut Ui, &mut T) -> Result<()>) -> Result<()> {
        let indicator_col = crate::theme::FIREFLY_PINK_DEEP;
        let selected_text_col = WHITE;
        let content_bg = Color::new(0.196, 0.122, 0.196, 0.95);

        self.render_plain(ui, WHITE, true, cr);

        let x_left = self.x_left.now(t);
        let x_right = self.x_right.now(t);
        let tab_y = cr.bottom() - Self::TAB_HEIGHT;

        let indicator = Rect::new(x_left, tab_y, x_right - x_left, Self::TAB_HEIGHT)
            .nonuniform_feather(-0.012, 0.007);
        rounded_rect_shadow(
            ui,
            indicator,
            &ShadowConfig {
                radius: 0.008,
                base: 0.5,
                ..Default::default()
            },
        );
        ui.fill_path(&indicator.rounded(0.008), indicator_col);

        ui.scissor(indicator, |ui| self.render_plain(ui, selected_text_col, false, cr));

        let content_cr = Rect::new(cr.x, cr.y, cr.w, tab_y - cr.y);
        ui.fill_path(&content_cr.rounded(0.005), content_bg);
        ui.scissor::<Result<()>>(content_cr, |ui| {
            let p = if get_data().prefer_reduced_motion {
                1.
            } else {
                self.content_progress.now(t)
            };
            if p < 1. {
                ui.scope(|ui| {
                    let dy = Self::CONTENT_DY * p;
                    ui.dy(if self.prev_go_up { -dy } else { dy });
                    ui.alpha(1. - p, |ui| f(ui, &mut self.items[self.prev].value))
                })?;
            }

            ui.scope(|ui| {
                let dy = Self::CONTENT_DY * (1. - p);
                ui.dy(if self.prev_go_up { dy } else { -dy });
                ui.alpha(p, |ui| f(ui, &mut self.items[self.selected].value))
            })?;

            Ok(())
        })?;

        Ok(())
    }
}
