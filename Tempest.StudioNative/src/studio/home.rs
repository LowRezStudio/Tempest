//! The start page of an open game folder: what to change (each task opens the
//! view made for it), the mod the changes go into, and how the Studio works.

use gpui::{div, prelude::*, px, AnyElement, Context, Hsla, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{commands, folder_label, ui, Studio, View};

/// A task of the start page: its icon and colour, what it is, and where it leads.
struct Task {
    id: &'static str,
    icon: IconName,
    title: &'static str,
    detail: &'static str,
    go: fn(&mut Studio, &mut Window, &mut Context<Studio>),
}

const TASKS: [Task; 6] = [
    Task { id: "task-look", icon: IconName::Shirt, title: "Change a champion's look", detail: "Skins, weapons and models: swap textures, materials and meshes.", go: |s, w, cx| s.open_palette("_assets_sf", w, cx) },
    Task { id: "task-data", icon: IconName::SlidersHorizontal, title: "Change stats, cards and items", detail: "Damage, cooldowns, talents, loadout cards, prices, loot.", go: |s, w, cx| s.set_view(View::Assembly, w, cx) },
    Task { id: "task-sound", icon: IconName::AudioLines, title: "Replace voice lines, music and sounds", detail: "Listen to every sound of the game and swap in your own.", go: |s, w, cx| s.set_view(View::Audio, w, cx) },
    Task { id: "task-map", icon: IconName::Map, title: "Edit a map", detail: "Move props, change lighting, collision, paths and cutscenes.", go: |s, w, cx| s.set_view(View::Map, w, cx) },
    Task { id: "task-ui", icon: IconName::LayoutGrid, title: "Change the menus and HUD", detail: "The interface movies: texts, images and layouts.", go: |s, w, cx| s.open_palette("ui_sf", w, cx) },
    Task { id: "task-new", icon: IconName::Sparkles, title: "Add something new", detail: "A new skin, ability, card, item or even a champion, step by step.", go: |s, w, cx| s.open_wizard(None, None, w, cx) },
];

impl Studio {
    pub(super) fn render_home(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tones: [Hsla; 6] = [theme.magenta, theme.yellow, theme.green, theme.blue, theme.cyan, theme.red];
        let mut tasks = v_flex().gap_1();
        for (k, t) in TASKS.iter().enumerate() {
            let go = t.go;
            tasks = tasks.child(
                h_flex()
                    .id(t.id)
                    .relative()
                    .gap_3()
                    .px_3()
                    .py_2p5()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, window, cx| go(this, window, cx)))
                    .child(div().flex_none().size(px(36.0)).rounded_md().bg(tones[k].opacity(0.14)).flex().items_center().justify_center().child(Icon::new(t.icon.clone()).text_color(tones[k])))
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(t.title)).child(div().text_xs().text_color(theme.muted_foreground).child(t.detail)))
                    .child(Icon::new(IconName::ChevronRight).small().text_color(theme.muted_foreground))
                    .child(crate::driver::probe(t.id)),
            );
        }
        let left = v_flex()
            .flex_1()
            .min_w(px(340.0))
            .max_w(px(560.0))
            .gap_2()
            .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).child("What do you want to change?"))
            .child(div().pb_2().text_sm().text_color(theme.muted_foreground).child(format!("Paladins in {}. Pick a task, or press {} to find anything by name.", self.dir.as_deref().map(folder_label).unwrap_or_default(), commands::key_hint("⌘K"))))
            .child(tasks);
        let right = v_flex().w(px(300.0)).flex_none().gap_5().child(self.render_home_mod(cx)).child(self.render_home_steps(cx)).children(self.render_home_recent(cx));
        div().id("home").size_full().overflow_y_scroll().child(h_flex().w_full().justify_center().items_start().flex_wrap().gap_10().px_8().py_10().child(left).child(right))
    }

    /// The mod the changes go into: its name and state, and what to do with it.
    fn render_home_mod(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let changes = self.unsaved_total(cx);
        let (title, body) = match &self.project {
            Some(p) => (p.name.clone(), format!("{} · {} change{} not built yet", self.project_saving.label(), changes, if changes == 1 { "" } else { "s" })),
            None if changes > 0 => ("Unnamed mod".to_string(), format!("{changes} change{} kept as a draft. Name it to give it its own folder.", if changes == 1 { "" } else { "s" })),
            None => ("No mod yet".to_string(), "Change anything and it is kept here automatically, as your mod.".to_string()),
        };
        let mut actions = h_flex().gap_2().pt_1();
        if self.project.is_none() && changes > 0 {
            actions = actions.child(Button::new("home-name").small().label("Name this mod…").on_click(cx.listener(|this, _, window, cx| this.new_project_dialog(window, cx))));
        }
        if self.project.is_some() || changes > 0 {
            actions = actions.child(Button::new("home-build").small().primary().icon(Icon::new(IconName::Hammer)).label("Build mod").on_click(cx.listener(|this, _, window, cx| this.set_view(View::Mod, window, cx))));
        } else {
            actions = actions.child(Button::new("home-open-mod").small().ghost().icon(Icon::new(IconName::Briefcase)).label("Open a mod…").on_click(cx.listener(|this, _, _, cx| this.pick_project(cx))));
        }
        v_flex()
            .gap_1()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(h_flex().gap_2().child(Icon::new(IconName::Briefcase).small().text_color(theme.muted_foreground)).child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Your mod")))
            .child(div().text_base().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
            .child(ui::hint(body, &theme))
            .child(actions)
            .into_any_element()
    }

    /// How changes become a mod, in three steps.
    fn render_home_steps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let step = |n: &'static str, title: &'static str, body: &'static str| {
            h_flex()
                .items_start()
                .gap_3()
                .child(div().flex_none().size(px(20.0)).rounded_full().border_1().border_color(theme.border).flex().items_center().justify_center().text_xs().text_color(theme.muted_foreground).child(n))
                .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(title)).child(ui::hint(body, &theme)))
        };
        v_flex()
            .gap_3()
            .child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("How it works"))
            .child(step("1", "Change anything", "Textures, numbers, sounds, maps. The game's own files are never touched."))
            .child(step("2", "It is kept in your mod", "Every change is saved as you work; Undo and Redo take it back and forth."))
            .child(step("3", "Build mod", "Packs your changes into one file that Tempest Launcher installs and that you can share."))
            .into_any_element()
    }

    /// Mods opened before, to pick one up again.
    fn render_home_recent(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let current = self.project.as_ref().map(|p| p.file.clone());
        let files: Vec<String> = self.prefs.projects.iter().filter(|f| Some(*f) != current.as_ref() && std::path::Path::new(f).is_file()).take(4).cloned().collect();
        if files.is_empty() {
            return None;
        }
        let mut list = v_flex().gap_0p5();
        for (k, f) in files.into_iter().enumerate() {
            let name = std::path::Path::new(&f).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            list = list.child(
                h_flex()
                    .id(("home-recent", k))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .text_sm()
                    .on_click(cx.listener(move |this, _, _, cx| this.open_project(f.clone(), cx)))
                    .child(Icon::new(IconName::Briefcase).small().text_color(theme.muted_foreground))
                    .child(name),
            );
        }
        Some(v_flex().gap_1().child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Your other mods")).child(list).into_any_element())
    }
}

