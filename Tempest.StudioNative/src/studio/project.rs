//! Mod projects: a folder holding `<id>.twrk` (a linked workspace file with
//! every edit of the project), `mod-files/` (shipped as laid out: scripts/,
//! dlls/64/, files/…) and `builds/`. While a project is open every edit is
//! kept in its file, which saves itself shortly after a change; ⌘S saves
//! the project and writing into the game folder is a separate command.
//! Closing the project unloads its edits, so nothing of it can be saved
//! into the game folder by mistake; opening it again puts them back.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, App, Context, Entity, KeyDownEvent, PathPromptOptions, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, WindowExt};
use gpui_kit_assets::IconName;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{dialogs, file_name, Studio, Tone};
use crate::backend::EDITS;

/// How long the project waits after the last change before saving itself.
const SETTLE: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// The `.twrk` file (not stored in it: it is where it was read from).
    #[serde(skip)]
    pub file: String,
    pub name: String,
    pub id: String,
    pub author: String,
    pub version: String,
    /// The game files (names) edited while the project was open: what its mod lists.
    pub files: Vec<String>,
    /// The Mod view's details (listing, readme, added files…), as the cooker saves them.
    pub cooker: serde_json::Value,
}

/// Two paths name the same file (separators and case aside, like the Electron Studio's `samePath`).
pub fn same_path(a: &str, b: &str) -> bool {
    let key = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    key(a) == key(b)
}

impl Project {
    /// Remember the files edited now.
    pub fn note_files(&mut self, paths: impl IntoIterator<Item = String>) {
        for p in paths {
            let name = file_name(&p).to_ascii_lowercase();
            if !self.files.contains(&name) {
                self.files.push(name);
            }
        }
    }

    pub fn dir(&self) -> PathBuf {
        Path::new(&self.file).parent().map(Path::to_path_buf).unwrap_or_default()
    }

    pub fn mod_files(&self) -> PathBuf {
        self.dir().join("mod-files")
    }

    pub fn builds(&self) -> PathBuf {
        self.dir().join("builds")
    }
}

/// What to do once a project is closed (open another, quit…).
pub type AfterClose = Option<Box<dyn FnOnce(&mut Studio, &mut Context<Studio>)>>;

#[derive(Clone, Debug, PartialEq)]
pub enum Saving {
    Saved,
    Pending,
    Writing,
    Failed(String),
}

impl Saving {
    pub fn label(&self) -> &'static str {
        match self {
            Saving::Saved => "saved",
            Saving::Pending => "saving…",
            Saving::Writing => "saving…",
            Saving::Failed(_) => "not saved",
        }
    }
}

/// `My Cool Mod` → `my-cool-mod`.
pub fn id_of(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn documents() -> PathBuf {
    std::env::var_os("HOME").map(|h| Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

const README: &str = "This is a Tempest Studio mod project.\n\n\
- <id>.twrk keeps every edit of the project (open it with the Studio).\n\
- mod-files/ is shipped as laid out: scripts/ (Lua), dlls/64/ and dlls/32/ (code), files/<game path> (files that replace the game's).\n\
- builds/ holds the .tempest packages the Mod view builds.\n";

impl Studio {
    /// The project's autosave: once edits stop for a moment, write its file.
    pub(super) fn watch_project(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let alive = this.update(cx, |this, cx| {
                if this.project.is_none() || this.project_saving == Saving::Writing {
                    return;
                }
                let stamp = EDITS.load(Ordering::SeqCst);
                if stamp != this.project_seen {
                    this.project_seen = stamp;
                    this.project_changed = Some(Instant::now());
                    if this.project_saving != Saving::Pending {
                        this.project_saving = Saving::Pending;
                        cx.notify();
                    }
                } else if this.project_saving == Saving::Pending && this.project_changed.is_some_and(|t| t.elapsed() >= SETTLE) {
                    this.save_project(cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Write the project file now (a linked workspace file with the project's details).
    pub(super) fn save_project(&mut self, cx: &mut Context<Self>) {
        let mut edited: Vec<String> = self.dirty.iter().cloned().collect();
        edited.extend(self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default());
        let Some(project) = &mut self.project else { return };
        project.note_files(edited);
        let p = project.clone();
        if self.dir.is_none() {
            return;
        }
        self.project_saving = Saving::Writing;
        let stamp = EDITS.load(Ordering::SeqCst);
        let call = self.backend.call("twrk.save", json!({ "out": p.file, "mode": "linked", "state": self.twrk_state() }));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(_) => {
                        // Edits made while it was written are saved next time.
                        this.project_saving = if EDITS.load(Ordering::SeqCst) == stamp { Saving::Saved } else { Saving::Pending };
                    }
                    Err(e) => {
                        let why = super::ui::explain_error(&e);
                        this.toast(Tone::Error, format!("Could not save the project: {why}"), cx);
                        this.project_saving = Saving::Failed(why);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Changed details (name, version…) count as an edit of the project.
    pub(crate) fn set_project_details(&mut self, name: String, id: String, author: String, version: String, cooker: serde_json::Value, cx: &mut Context<Self>) {
        let Some(p) = &mut self.project else { return };
        if (p.name.as_str(), p.id.as_str(), p.author.as_str(), p.version.as_str(), &p.cooker) == (name.as_str(), id.as_str(), author.as_str(), version.as_str(), &cooker) {
            return;
        }
        (p.name, p.id, p.author, p.version, p.cooker) = (name, id, author, version, cooker);
        EDITS.fetch_add(1, Ordering::SeqCst);
        cx.notify();
    }

    /// A project was read from `file` (`state.project` of a workspace file).
    pub(super) fn adopt_project(&mut self, file: &str, state: &serde_json::Value) {
        let Some(mut p) = state.get("project").and_then(|p| serde_json::from_value::<Project>(p.clone()).ok()) else { return };
        p.file = file.to_string();
        self.project = Some(p);
        self.project_saving = Saving::Saved;
        self.project_seen = EDITS.load(Ordering::SeqCst);
        self.cooker = None;
        self.prefs.projects.retain(|f| f != file);
        self.prefs.projects.insert(0, file.to_string());
        self.prefs.projects.truncate(12);
        self.prefs.save();
    }

    pub(super) fn new_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dir.is_none() {
            self.toast(Tone::Info, "Open the game folder the mod is for first", cx);
            return;
        }
        let input = |window: &mut Window, cx: &mut Context<Self>, v: &str, ph: &str| {
            let (v, ph) = (v.to_string(), ph.to_string());
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(ph);
                s.set_value(v, window, cx);
                s
            })
        };
        let game = self.dir.clone().unwrap_or_default();
        let draft_meta = match &self.cooker {
            Some(c) => c.read(cx).draft_json(cx),
            None => super::cooker::read_draft_meta(&game),
        };
        let draft_str = |k: &str| draft_meta[k].as_str().unwrap_or_default().trim().to_string();
        let draft_name = draft_str("name");
        let draft_id = draft_str("id");
        let draft_author = draft_str("author");
        let draft_version = draft_str("version");
        let init_id_ph = {
            let made = id_of(&draft_name);
            if made.is_empty() { "made from the name".to_string() } else { made }
        };
        let name = input(window, cx, &draft_name, "My mod");
        let id = input(window, cx, &draft_id, &init_id_ph);
        let author = input(window, cx, &draft_author, "your name");
        let version = input(window, cx, if draft_version.is_empty() { "1.0.0" } else { &draft_version }, "1.0.0");
        let base = documents().join("Tempest Mods");
        let location = input(window, cx, &base.to_string_lossy(), "folder the project folder goes in");
        let changes = self.unsaved_total(cx);
        let me = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        // The id follows the name until it is typed.
        let id2 = id.clone();
        cx.subscribe_in(&name, window, move |_, name: &Entity<InputState>, e: &InputEvent, window, cx| {
            if matches!(e, InputEvent::Change) {
                let made = id_of(&name.read(cx).value());
                id2.update(cx, |s, cx| s.set_placeholder(if made.is_empty() { "made from the name".to_string() } else { made }, window, cx));
            }
        })
        .detach();
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                name.update(cx, |s, cx| s.focus(window, cx));
            }
            let theme = cx.theme().clone();
            let row = |label: &'static str, i: &Entity<InputState>| h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label)).child(div().flex_1().min_w_0().child(Input::new(i).small()));
            let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
            let mod_id = {
                let typed = text(&id, cx);
                if typed.is_empty() { id_of(&text(&name, cx)) } else { typed }
            };
            let folder = Path::new(&text(&location, cx)).join(if mod_id.is_empty() { "<id>" } else { &mod_id }).to_string_lossy().to_string();
            let ok = !mod_id.is_empty();
            let (me, n2, i2, a2, v2, l2) = (me.clone(), name.clone(), id.clone(), author.clone(), version.clone(), location.clone());
            let draft_for_create = draft_meta.clone();
            let l3 = location.clone();
            dialog
                .title("Name your mod")
                .w(px(620.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(if changes > 0 {
                            format!("Your {changes} change{} go into it. The mod gets its own folder and saves itself as you work; the game's own files stay untouched.", if changes == 1 { "" } else { "s" })
                        } else {
                            "The mod gets its own folder and saves itself as you work; the game's own files stay untouched.".to_string()
                        }))
                        .child(row("Name", &name))
                        .child(row("Id", &id))
                        .child(row("Author", &author))
                        .child(row("Version", &version))
                        .child(h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child("Location")).child(div().flex_1().min_w_0().child(Input::new(&location).small())).child(Button::new("np-browse").small().ghost().label("Choose…").on_click(move |_, window, cx| {
                            let pick = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Where the project folder goes".into()) });
                            let l3 = l3.clone();
                            let handle = window.window_handle();
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(mut p))) = pick.await {
                                    if let Some(d) = p.pop() {
                                        let _ = cx.update_window(handle, |_, window, cx| l3.update(cx, |s, cx| s.set_value(d.to_string_lossy().to_string(), window, cx)));
                                    }
                                }
                            })
                            .detach();
                        })))
                        .child(super::ui::hint(format!("Creates {folder} (with {mod_id}.twrk, mod-files/ and builds/), for the game in {}.", super::folder_label(&game)), &theme)),
                )
                .footer(dialogs::footer("Create the mod", false, ok, move |window, cx| {
                    let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
                    let name = text(&n2, cx);
                    let typed = text(&i2, cx);
                    let id = if typed.is_empty() { id_of(&name) } else { typed };
                    let final_name = if name.is_empty() { id.clone() } else { name };
                    let author = text(&a2, cx);
                    let version = {
                        let v = text(&v2, cx);
                        if v.is_empty() { "1.0.0".to_string() } else { v }
                    };
                    let mut cooker = if draft_for_create.is_object() { draft_for_create.clone() } else { json!({}) };
                    cooker["name"] = json!(&final_name);
                    cooker["id"] = json!(&id);
                    cooker["author"] = json!(&author);
                    cooker["version"] = json!(&version);
                    let p = Project { file: String::new(), name: final_name, id, author, version, files: Vec::new(), cooker };
                    let location = text(&l2, cx);
                    window.close_dialog(cx);
                    let _ = me.update(cx, |this, cx| this.create_project(p, PathBuf::from(location), window, cx));
                }))
        });
    }

    pub(super) fn create_project(&mut self, mut p: Project, location: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let dir = location.join(&p.id);
        let file = dir.join(format!("{}.twrk", p.id));
        if file.exists() {
            self.toast(Tone::Warning, format!("{} already exists: open it instead (Open project…)", file.display()), cx);
            return;
        }
        let made = std::fs::create_dir_all(dir.join("mod-files").join("scripts")).and_then(|_| std::fs::create_dir_all(dir.join("builds"))).and_then(|_| std::fs::write(dir.join("README.txt"), README.replace("<id>", &p.id)));
        if let Err(e) = made {
            self.toast(Tone::Error, format!("Could not create {}: {}", dir.display(), super::ui::explain_error(&e.to_string())), cx);
            return;
        }
        p.file = file.to_string_lossy().to_string();
        let start = move |this: &mut Studio, cx: &mut Context<Studio>| {
            let state = json!({ "project": p });
            this.adopt_project(&p.file, &state);
            this.save_project(cx);
            this.toast(Tone::Success, format!("{} created: your changes are kept in it", p.name), cx);
        };
        // The draft's changes become the mod's (the dialog said so).
        let _ = window;
        start(self, cx);
    }

    pub(super) fn pick_project(&mut self, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open a mod project (.twrk)".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut p))) = files.await else { return };
            if let Some(f) = p.pop() {
                let _ = this.update(cx, |this, cx| this.open_project(f.to_string_lossy().to_string(), cx));
            }
        })
        .detach();
    }

    /// Open a project (closing the current one first, its edits saved).
    pub(crate) fn open_project(&mut self, file: String, cx: &mut Context<Self>) {
        if !Path::new(&file).is_file() {
            self.prefs.projects.retain(|f| *f != file);
            self.prefs.save();
            self.toast(Tone::Warning, format!("{file} is not there any more: removed from the recent projects"), cx);
            return;
        }
        if self.project.is_some() {
            self.close_project_now(true, Some(Box::new(move |this: &mut Studio, cx: &mut Context<Studio>| this.open_workspace_file(file.clone(), cx))), cx);
        } else {
            self.open_workspace_file(file, cx);
        }
    }

    /// Close the project: ask when it has changes not written yet (including unapplied UI movie or particle edits).
    pub(super) fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.project.clone() else { return };
        let me = cx.entity().downgrade();
        let unapplied_movies = self.unapplied_movie_count(cx);
        let unapplied_particles = self.unapplied_particle_count();
        let unapplied = unapplied_movies + unapplied_particles;
        let pending = self.project_saving != Saving::Saved || unapplied > 0;
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (m1, m2) = (me.clone(), me.clone());
            let footer = h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .child(Button::new("cp-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                .when(pending, |d| {
                    d.child(Button::new("cp-discard").danger().label("Discard and close").on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let _ = m1.update(cx, |this, cx| this.close_project_now(false, None, cx));
                    }))
                })
                .child(Button::new("cp-close").primary().label(if pending { "Save and close" } else { "Close" }).on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    let _ = m2.update(cx, |this, cx| this.close_project_now(true, None, cx));
                }));
            dialog
                .title(format!("Close the project {}?", p.name))
                .w(px(520.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child("Its edits are unloaded from the Studio (they stay in the project file and come back when you open it), so nothing of it ends up in the game folder by mistake."))
                        .when(unapplied_movies > 0, |d| {
                            d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(format!(
                                "{unapplied_movies} UI movie{} unapplied stage edits. Save and close applies {} into the project first; Discard and close drops {}.",
                                if unapplied_movies == 1 { " has" } else { "s have" },
                                if unapplied_movies == 1 { "it" } else { "them" },
                                if unapplied_movies == 1 { "it" } else { "them" }
                            )))
                        })
                        .when(unapplied_particles > 0, |d| {
                            d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(
                                "Unapplied particle curve edits: Save and close applies them into the project first; Discard and close drops them.",
                            ))
                        }),
                )
                .footer(footer)
        });
    }

    /// Save (or not), unload every edit, forget the project, then `then`.
    pub(super) fn close_project_now(&mut self, save: bool, then: AfterClose, cx: &mut Context<Self>) {
        let movie_applies = if save { self.pending_movie_applies(cx) } else { Vec::new() };
        let particle_applies = if save { self.pending_particle_applies() } else { Vec::new() };
        let mut edited: Vec<String> = self.dirty.iter().cloned().collect();
        edited.extend(movie_applies.iter().map(|(path, ..)| path.clone()));
        edited.extend(particle_applies.iter().map(|(path, ..)| path.clone()));
        edited.extend(self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default());
        if let Some(project) = &mut self.project {
            project.note_files(edited);
        }
        let Some(p) = self.project.clone() else { return };
        let state = self.twrk_state();
        let mut packages: Vec<String> = self.dirty.iter().cloned().collect();
        for (path, ..) in &movie_applies {
            if !packages.iter().any(|x| same_path(x, path)) {
                packages.push(path.clone());
            }
        }
        for (path, ..) in &particle_applies {
            if !packages.iter().any(|x| same_path(x, path)) {
                packages.push(path.clone());
            }
        }
        let docs = self.assembly.is_some() || self.applied_docs;
        let backend = self.backend.clone();
        self.set_status(format!("Closing {}…", p.name), cx);
        cx.spawn(async move |this, cx| {
            if save {
                for (path, index, edits) in &movie_applies {
                    if let Err(e) = backend.call("swf.apply", json!({ "path": path, "index": index, "edits": edits })).await {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not apply UI movie edits before closing: {}", super::ui::explain_error(&e)), cx));
                        return;
                    }
                }
                for (path, index, d) in &particle_applies {
                    if let Err(e) = backend.call("particle.setTable", json!({ "path": path, "index": index, "prop": d.prop, "table": d.table(), "timeScale": d.time_scale, "startTime": d.start_time })).await {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not apply particle curve edits before closing: {}", super::ui::explain_error(&e)), cx));
                        return;
                    }
                }
                if let Err(e) = backend.call("twrk.save", json!({ "out": p.file, "mode": "linked", "state": state })).await {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not save the project, so it stays open: {}", super::ui::explain_error(&e)), cx));
                    return;
                }
            }
            for path in &packages {
                let _ = backend.call("package.discard", json!({ "path": path })).await;
            }
            if docs {
                if let Ok(r) = backend.call("mcts.open", json!({})).await {
                    for d in r.json["docs"].as_array().into_iter().flatten().filter(|d| d["dirty"].as_bool() == Some(true)) {
                        let _ = backend.call("mcts.discard", json!({ "doc": d["id"] })).await;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.project = None;
                this.project_saving = Saving::Saved;
                this.applied_docs = false;
                this.dirty.clear();
                this.movies.clear();
                this.particles = None;
                this.focus_mode = false;
                this.cooker = None;
                if let Some(a) = &this.assembly {
                    a.update(cx, |a, cx| a.reopen(cx));
                }
                // The open package without the project's edits.
                if let Some(path) = this.selected_package.take() {
                    let export = this.selected_export;
                    this.pending_export = export;
                    this.open_package(path, cx);
                }
                this.toast(Tone::Info, format!("Closed the project {}", p.name), cx);
                if let Some(then) = then {
                    then(this, cx);
                }
            });
        })
        .detach();
    }

    /// Switch or pick another game folder interactively, protecting unsaved changes and unapplied UI movie stage edits.
    pub(super) fn switch_workspace_or_ask(&mut self, next: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let normalized_next = next.as_ref().map(|d| {
            let cooked = Path::new(d).join("ChaosGame").join("CookedPCConsole");
            if cooked.is_dir() { cooked.to_string_lossy().to_string() } else { d.clone() }
        });
        if let (Some(cur), Some(target)) = (self.dir.as_deref(), normalized_next.as_deref()) {
            if same_path(cur, target) {
                return;
            }
        }
        type StudioCont = Box<dyn FnOnce(&mut Studio, &mut Context<Studio>)>;
        let go_next = |next: Option<String>| -> StudioCont {
            Box::new(move |this, cx| match next {
                Some(d) => this.open_workspace(d, cx),
                None => this.pick_workspace(cx),
            })
        };
        let unapplied_movies = self.unapplied_movie_count(cx);
        let unapplied_particles = self.unapplied_particle_count();
        let unapplied = unapplied_movies + unapplied_particles;
        let changes = self.unsaved_total(cx);
        if self.dir.is_none() || (changes == 0 && self.project.is_none()) {
            go_next(next)(self, cx);
            return;
        }
        if let Some(p) = self.project.clone() {
            if self.project_saving == Saving::Saved && unapplied == 0 {
                self.close_project_now(true, Some(go_next(next)), cx);
                return;
            }
            let me = cx.entity().downgrade();
            window.open_dialog(cx, move |dialog, _, cx| {
                let theme = cx.theme().clone();
                let (m1, m2, n1, n2) = (me.clone(), me.clone(), next.clone(), next.clone());
                let footer = h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("sw-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("sw-discard").danger().label("Discard and switch").on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let n = n1.clone();
                        let _ = m1.update(cx, |this, cx| this.close_project_now(false, Some(go_next(n)), cx));
                    }))
                    .child(Button::new("sw-save").primary().label("Save and switch").on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let n = n2.clone();
                        let _ = m2.update(cx, |this, cx| this.close_project_now(true, Some(go_next(n)), cx));
                    }));
                dialog
                    .title("Switch game folders?")
                    .w(px(520.0))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(format!("Save the project {} before opening another game folder?", p.name)))
                            .when(unapplied_movies > 0, |d| {
                                d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(format!(
                                    "{unapplied_movies} UI movie{} unapplied stage edits: Save and switch applies {} into the project first.",
                                    if unapplied_movies == 1 { " has" } else { "s have" },
                                    if unapplied_movies == 1 { "it" } else { "them" }
                                )))
                            })
                            .when(unapplied_particles > 0, |d| {
                                d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(
                                    "Unapplied particle curve edits: Save and switch applies them into the project first.",
                                ))
                            }),
                    )
                    .footer(footer)
            });
            return;
        }
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (m1, m2, n1, n2) = (me.clone(), me.clone(), next.clone(), next.clone());
            let footer = h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .child(Button::new("sw-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("sw-discard").danger().label("Discard and switch").on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    let n = n1.clone();
                    let _ = m1.update(cx, |this, cx| this.finish_draft_and_switch(false, n, cx));
                }))
                .child(Button::new("sw-keep").primary().label("Keep draft & switch").on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    let n = n2.clone();
                    let _ = m2.update(cx, |this, cx| this.finish_draft_and_switch(true, n, cx));
                }));
            dialog
                .title("Switch game folders?")
                .w(px(520.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(format!(
                            "You have {changes} change{} in this game folder. Keep them as a recovery draft (offered back next time you open this folder) or discard them?",
                            if changes == 1 { "" } else { "s" }
                        )))
                        .when(unapplied_movies > 0, |d| {
                            d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(format!(
                                "{unapplied_movies} UI movie{} unapplied stage edits: Keep draft & switch applies {} so {} kept too.",
                                if unapplied_movies == 1 { " has" } else { "s have" },
                                if unapplied_movies == 1 { "it" } else { "them" },
                                if unapplied_movies == 1 { "it is" } else { "they are" }
                            )))
                        })
                        .when(unapplied_particles > 0, |d| {
                            d.child(div().text_xs().text_color(theme.warning).whitespace_normal().child(
                                "Unapplied particle curve edits: Keep draft & switch applies them so they are kept too.",
                            ))
                        }),
                )
                .footer(footer)
        });
    }

    fn finish_draft_and_switch(&mut self, keep: bool, next: Option<String>, cx: &mut Context<Self>) {
        let cur_dir = self.dir.clone().unwrap_or_default();
        let rec_file = Self::recovery_file(&cur_dir);
        let movie_applies = if keep { self.pending_movie_applies(cx) } else { Vec::new() };
        let particle_applies = if keep { self.pending_particle_applies() } else { Vec::new() };
        let state = self.twrk_state();
        let mut packages: Vec<String> = self.dirty.iter().cloned().collect();
        for (path, ..) in &movie_applies {
            if !packages.iter().any(|x| same_path(x, path)) {
                packages.push(path.clone());
            }
        }
        for (path, ..) in &particle_applies {
            if !packages.iter().any(|x| same_path(x, path)) {
                packages.push(path.clone());
            }
        }
        let docs = self.assembly.is_some() || self.applied_docs;
        let backend = self.backend.clone();
        self.discard_unapplied_movies(None, cx);
        cx.spawn(async move |this, cx| {
            if keep {
                for (path, index, edits) in &movie_applies {
                    if let Err(e) = backend.call("swf.apply", json!({ "path": path, "index": index, "edits": edits })).await {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not apply UI movie edits: {}", super::ui::explain_error(&e)), cx));
                        return;
                    }
                }
                for (path, index, d) in &particle_applies {
                    if let Err(e) = backend.call("particle.setTable", json!({ "path": path, "index": index, "prop": d.prop, "table": d.table(), "timeScale": d.time_scale, "startTime": d.start_time })).await {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not apply particle curve edits: {}", super::ui::explain_error(&e)), cx));
                        return;
                    }
                }
                if let Some(file) = &rec_file {
                    if let Some(parent) = file.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = backend.call("twrk.save", json!({ "out": file.to_string_lossy(), "mode": "linked", "state": state })).await {
                        let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not keep your draft: {}", super::ui::explain_error(&e)), cx));
                        return;
                    }
                }
            } else if let Some(file) = &rec_file {
                let _ = std::fs::remove_file(file);
            }
            for path in &packages {
                let _ = backend.call("package.discard", json!({ "path": path })).await;
            }
            if docs {
                if let Ok(r) = backend.call("mcts.open", json!({})).await {
                    for d in r.json["docs"].as_array().into_iter().flatten().filter(|d| d["dirty"].as_bool() == Some(true)) {
                        let _ = backend.call("mcts.discard", json!({ "doc": d["id"] })).await;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.dirty.clear();
                this.applied_docs = false;
                this.movies.clear();
                this.particles = None;
                this.focus_mode = false;
                match next {
                    Some(d) => this.open_workspace(d, cx),
                    None => this.pick_workspace(cx),
                }
            });
        })
        .detach();
    }

    /// Unapplied movie editors sorted by `(package_path, export_index)`:
    /// `(package_path, export_index, movie_name, edit_count, is_applying, Entity<MovieEditor>)`.
    pub(crate) fn unapplied_movies(&self, cx: &App) -> Vec<(String, usize, String, usize, bool, Entity<super::swf::MovieEditor>)> {
        let mut out: Vec<_> = self
            .movies
            .values()
            .filter_map(|m| {
                let r = m.read(cx);
                let n = r.unapplied_count();
                (n > 0).then(|| {
                    let (path, idx, name) = r.movie_target();
                    (path.to_string(), idx, name.to_string(), n, r.is_applying(), m.clone())
                })
            })
            .collect();
        out.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        out
    }

    pub(crate) fn unapplied_movie_count(&self, cx: &App) -> usize {
        self.movies.values().filter(|m| m.read(cx).has_unapplied()).count()
    }

    pub(crate) fn pending_movie_applies(&self, cx: &App) -> Vec<(String, usize, serde_json::Value)> {
        self.movies
            .values()
            .filter_map(|m| {
                let r = m.read(cx);
                r.has_unapplied().then(|| (r.path.clone(), r.index, r.pending()))
            })
            .collect()
    }

    /// Discard unapplied staged edits in every movie editor (and matching particle curve drafts).
    pub(crate) fn discard_unapplied_movies(&mut self, only_path: Option<&str>, cx: &mut Context<Self>) {
        let movies: Vec<_> = self
            .movies
            .iter()
            .filter(|((p, _), _)| only_path.is_none_or(|target| same_path(p, target)))
            .map(|(_, m)| m.clone())
            .collect();
        for m in movies {
            m.update(cx, |m, cx| m.discard_unapplied(cx));
        }
        if self.particles.as_ref().is_some_and(|p| only_path.is_none_or(|target| same_path(&p.path, target))) {
            self.discard_particle_drafts(cx);
        }
    }

    /// Keep `Cooker` synced with the active `MovieEditor` entities and unapplied particle curve drafts.
    pub(crate) fn sync_cooker(&self, cx: &mut Context<Self>) {
        if let Some(c) = self.cooker.clone() {
            let movies: Vec<_> = self.movies.values().cloned().collect();
            let particles = self.unapplied_particle_curves();
            c.update(cx, |c, cx| c.sync_unapplied(movies, particles, cx));
        }
    }

    pub(crate) fn unsaved_total(&self, cx: &App) -> usize {
        self.dirty.len()
            + self.assembly.as_ref().map(|a| a.read(cx).unsaved_count()).unwrap_or(usize::from(self.applied_docs))
            + self.unapplied_movie_count(cx)
            + self.unapplied_particle_count()
    }

    /// Open the `Changes (n)` modal: delegates to `show_unsaved` unless there are unapplied UI movie or particle edits.
    pub(super) fn show_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unapplied_movie_count(cx) == 0 && self.unapplied_particle_count() == 0 {
            self.show_unsaved(window, cx);
            return;
        }
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let max_w = window.viewport_size().width * 0.94;
            let theme = cx.theme().clone();
            let Some(studio) = me.upgrade() else { return dialog };
            let (mut files, assembly, movies, particle) = {
                let s = studio.read(cx);
                let files: Vec<String> = s.dirty.iter().cloned().collect();
                let assembly = s.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default();
                let movies = s.unapplied_movies(cx);
                let particle = s.unapplied_particle_item();
                (files, assembly, movies, particle)
            };
            files.sort();
            let mut list = v_flex().gap_1p5();
            if let Some(pt) = &particle {
                let (me_open, me_key, me_apply, me_discard) = (me.clone(), me.clone(), me.clone(), me.clone());
                let (pkg_open, pkg_key) = (pt.path.clone(), pt.path.clone());
                let export = pt.export;
                let badge_text = format!("{} unapplied curve edit{}", pt.curves, if pt.curves == 1 { "" } else { "s" });
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .items_center()
                        .child(
                            h_flex()
                                .id("open-pt")
                                .focusable()
                                .tab_index(0)
                                .flex_1()
                                .min_w_0()
                                .gap_2()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.list_hover))
                                .focus_visible(|s| s.border_color(theme.ring))
                                .child(Icon::new(IconName::Sparkles).xsmall().text_color(theme.warning))
                                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(pt.name.clone()))
                                .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().font_family(super::ui::mono()).text_color(theme.muted_foreground).child(file_name(&pt.path)))
                                .on_key_down(move |e: &KeyDownEvent, window, cx| {
                                    if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        window.close_dialog(cx);
                                        let place = super::Place::package(pkg_key.clone(), export);
                                        let _ = me_key.update(cx, |this, cx| {
                                            this.record(place.clone());
                                            this.go(place, window, cx);
                                        });
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let place = super::Place::package(pkg_open.clone(), export);
                                    let _ = me_open.update(cx, |this, cx| {
                                        this.record(place.clone());
                                        this.go(place, window, cx);
                                    });
                                }),
                        )
                        .child(super::ui::badge(Some(IconName::TriangleAlert), badge_text, theme.warning, &theme))
                        .child(
                            Button::new("apply-pt")
                                .xsmall()
                                .label("Apply")
                                .tooltip("Write these particle curve edits into the module so they go into your mod")
                                .on_click(move |_, window, cx| {
                                    let _ = me_apply.update(cx, |this, cx| this.apply_particle_drafts(cx));
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("discard-pt")
                                .xsmall()
                                .ghost()
                                .label("Discard")
                                .tooltip("Drop the unapplied particle curve edits")
                                .on_click(move |_, window, cx| {
                                    let empty = me_discard
                                        .update(cx, |this, cx| {
                                            this.discard_particle_drafts(cx);
                                            this.unsaved_total(cx) == 0
                                        })
                                        .unwrap_or(true);
                                    if empty {
                                        window.close_dialog(cx);
                                    } else {
                                        window.refresh();
                                    }
                                }),
                        ),
                );
            }
            for (k, (pkg, idx, name, edits, applying, ent)) in movies.iter().enumerate() {
                let (me_open, pkg_open, pkg_key) = (me.clone(), pkg.clone(), pkg.clone());
                let me_key = me.clone();
                let index = *idx;
                let (ent_apply, ent_discard, me_discard) = (ent.clone(), ent.clone(), me.clone());
                let badge_text = format!("{edits} unapplied stage edit{}", if *edits == 1 { "" } else { "s" });
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .items_center()
                        .child(
                            h_flex()
                                .id(("open-mv", k))
                                .focusable()
                                .tab_index(0)
                                .flex_1()
                                .min_w_0()
                                .gap_2()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.list_hover))
                                .focus_visible(|s| s.border_color(theme.ring))
                                .child(Icon::new(IconName::Layers).xsmall().text_color(theme.warning))
                                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(name.clone()))
                                .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().font_family(super::ui::mono()).text_color(theme.muted_foreground).child(file_name(pkg)))
                                .on_key_down(move |e: &KeyDownEvent, window, cx| {
                                    if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        window.close_dialog(cx);
                                        let place = super::Place::package(pkg_key.clone(), Some(index));
                                        let _ = me_key.update(cx, |this, cx| {
                                            this.record(place.clone());
                                            this.go(place, window, cx);
                                        });
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let place = super::Place::package(pkg_open.clone(), Some(index));
                                    let _ = me_open.update(cx, |this, cx| {
                                        this.record(place.clone());
                                        this.go(place, window, cx);
                                    });
                                }),
                        )
                        .child(super::ui::badge(Some(IconName::TriangleAlert), badge_text, theme.warning, &theme))
                        .child(
                            Button::new(("apply-mv", k))
                                .xsmall()
                                .label(if *applying { "Applying…" } else { "Apply" })
                                .disabled(*applying)
                                .tooltip("Apply these stage edits into the package so they go into your mod")
                                .on_click(move |_, window, cx| {
                                    ent_apply.update(cx, |m, cx| m.apply(cx));
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new(("discard-mv", k))
                                .xsmall()
                                .ghost()
                                .label("Discard")
                                .disabled(*applying)
                                .tooltip("Drop the unapplied stage edits of this movie")
                                .on_click(move |_, window, cx| {
                                    ent_discard.update(cx, |m, cx| m.discard_unapplied(cx));
                                    if me_discard.upgrade().is_none_or(|s| s.read(cx).unsaved_total(cx) == 0) {
                                        window.close_dialog(cx);
                                    } else {
                                        window.refresh();
                                    }
                                }),
                        ),
                );
            }
            for f in &files {
                let (me1, me2, me3, f1, f2, f3) = (me.clone(), me.clone(), me.clone(), f.clone(), f.clone(), f.clone());
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().flex_1().min_w_0().child(file_name(f)))
                        .child(Button::new(SharedString::from(format!("open-{f}"))).xsmall().ghost().label("Open").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me1.update(cx, |this, cx| this.user_open_package(f1.clone(), cx));
                        }))
                        .child(Button::new(SharedString::from(format!("save-{f}"))).xsmall().ghost().label("Patch the game…").tooltip("Advanced, for quick tests: overwrite the game's own file with your changes").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me2.update(cx, |this, cx| this.save_packages(vec![f2.clone()], window, cx));
                        }))
                        .child(Button::new(SharedString::from(format!("discard-{f}"))).xsmall().ghost().label("Discard…").tooltip("Throw away the changes to this file").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me3.update(cx, |this, cx| this.discard(f3.clone(), window, cx));
                        })),
                );
            }
            for f in &assembly {
                let me = me.clone();
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().flex_1().min_w_0().child(file_name(f)))
                        .child(div().text_xs().text_color(theme.muted_foreground).child("game data"))
                        .child(Button::new(SharedString::from(format!("open-{f}"))).xsmall().ghost().label("Open").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me.update(cx, |this, cx| this.set_view(super::View::Assembly, window, cx));
                        })),
                );
            }
            let (me1, me2) = (me.clone(), me.clone());
            let file_count = files.len() + assembly.len();
            let movie_count = movies.len();
            let has_particles = particle.is_some();
            let summary = match (file_count, movie_count, has_particles) {
                (0, 0, true) => "Unapplied particle curve edits: click Apply to put them into your mod.".to_string(),
                (0, m, false) => format!("{m} UI movie{} unapplied stage edits: click Apply to put {} into your mod.", if m == 1 { " has" } else { "s have" }, if m == 1 { "it" } else { "them" }),
                (0, m, true) => format!("{m} UI movie{} and particle curve edits are unapplied: click Apply to put them into your mod.", if m == 1 { "" } else { "s" }),
                (f, 0, false) => format!("{f} changed file{}: they go into your mod when you build it. The game's own files are untouched.", if f == 1 { "" } else { "s" }),
                (f, 0, true) => format!("{f} changed file{} and unapplied particle curve edits. Click Apply so they are included when you build.", if f == 1 { "" } else { "s" }),
                (f, m, false) => format!(
                    "{f} changed file{} and {m} UI movie{} with unapplied stage edits. Click Apply on {} so {} included when you build.",
                    if f == 1 { "" } else { "s" },
                    if m == 1 { "" } else { "s" },
                    if m == 1 { "the movie" } else { "movies" },
                    if m == 1 { "it is" } else { "they are" }
                ),
                (f, m, true) => format!(
                    "{f} changed file{}, {m} UI movie{} and particle curve edits with unapplied changes. Click Apply so they are included when you build.",
                    if f == 1 { "" } else { "s" },
                    if m == 1 { "" } else { "s" },
                ),
            };
            let all = h_flex()
                .gap_2()
                .pb_2()
                .mb_1()
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().min_w_0().whitespace_normal().text_sm().text_color(theme.muted_foreground).child(summary))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(Button::new("discard-all").xsmall().danger().label("Discard all changes…").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let _ = me2.update(cx, |this, cx| this.run(super::commands::Cmd::DiscardAll, window, cx));
                        }))
                        .child(crate::driver::probe("discard-all")),
                );
            let save_empty_tip = if movie_count > 0 {
                "Apply your UI movie stage edits first"
            } else if has_particles {
                "Apply your particle curve edits first"
            } else {
                "Only game data changed: patch it from the Game data view's ⋯ menu"
            };
            let advanced = h_flex()
                .pt_2()
                .mt_1()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .child(super::ui::hint("Testing without building? You can overwrite the game's own files instead (advanced; only Verify game files puts them back).", &theme).flex_1().min_w_0())
                .child(Button::new("save-all").xsmall().ghost().label("Patch the game…").disabled(files.is_empty()).when(files.is_empty(), |b| b.tooltip(save_empty_tip)).on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    let _ = me1.update(cx, |this, cx| this.run(super::commands::Cmd::SaveAll, window, cx));
                }));
            dialog.title("Changes").w(px(620.0)).max_w(max_w).child(all).child(div().id("unsaved-list").max_h(px(420.0)).overflow_y_scroll().child(list)).child(advanced)
        });
    }

    /// The header's mod chip: the named mod (a project: name, save state, its
    /// menu), the unnamed draft the changes are kept in until then, or an
    /// invitation to start one.
    pub(super) fn render_project_chip(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui_component::menu::{DropdownMenu, PopupMenuItem};
        let dir = self.dir.clone()?;
        let theme = cx.theme().clone();
        let me = cx.entity().downgrade();
        let item = move |label: &str, f: fn(&mut Studio, &mut Window, &mut Context<Studio>)| {
            let me = me.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let Some(p) = self.project.clone() else {
            let changes = self.unsaved_total(cx);
            let draft_name = match &self.cooker {
                Some(c) => c.read(cx).meta.text("name", cx),
                None => super::cooker::read_draft_meta(&dir)["name"].as_str().unwrap_or_default().trim().to_string(),
            };
            let earlier = self.recovery_offer.is_some() && changes > 0;
            let earlier_what = self.recovery_offer.as_ref().map(|o| o.2.clone()).unwrap_or_default();
            let dot = div().size(px(7.0)).rounded_full().bg(if changes > 0 { theme.warning } else { theme.muted_foreground });
            let (label, tip) = match (!draft_name.is_empty(), changes > 0) {
                (true, true) => (
                    format!("{draft_name} (draft) · {changes} change{}", if changes == 1 { "" } else { "s" }),
                    "Your changes are kept as a draft, even if the Studio closes. Give the mod its own folder (⌘S), then build it.",
                ),
                (true, false) => (
                    format!("{draft_name} (draft)"),
                    "Draft mod details are kept for this game folder. Change anything to add files, or give the mod its own folder (⌘S).",
                ),
                (false, true) => (
                    format!("Unnamed mod · {changes} change{}", if changes == 1 { "" } else { "s" }),
                    "Your changes are kept as a draft, even if the Studio closes. Name the mod to give it its own folder, then build it.",
                ),
                (false, false) => (
                    "No mod yet".to_string(),
                    "Change anything and it becomes a draft mod; or start a named mod now.",
                ),
            };
            return Some(
                h_flex()
                    .gap_1()
                    .child(dot)
                    .child(
                        Button::new("project")
                            .small()
                            .ghost()
                            .icon(Icon::new(IconName::Briefcase))
                            .label(label)
                            .tooltip(tip)
                            .dropdown_menu(move |menu, _, _| {
                                let menu = menu.item(item("Name this mod…", |s, w, cx| s.new_project_dialog(w, cx))).item(item("Open a mod…", |s, _, cx| s.pick_project(cx)));
                                // An earlier session's changes, when new ones were made before answering.
                                let menu = if earlier {
                                    menu.separator().item(item(&format!("Add the earlier session's changes ({earlier_what})"), |s, _, cx| {
                                        if let Some((file, ..)) = s.recovery_offer.take() {
                                            s.open_workspace_file(file, cx);
                                        }
                                    }))
                                } else {
                                    menu
                                };
                                menu
                                    .separator()
                                    .item(item("Discard the draft…", |s, w, cx| s.run(super::Cmd::DiscardAll, w, cx)))
                            }),
                    )
                    .into_any_element(),
            );
        };
        let unapplied_movies = self.unapplied_movie_count(cx);
        let unapplied_particles = self.unapplied_particle_count();
        let unapplied = unapplied_movies + unapplied_particles;
        let state = self.project_saving.clone();
        let color = if unapplied > 0 {
            theme.warning
        } else {
            match state {
                Saving::Saved => theme.success,
                Saving::Failed(_) => theme.danger,
                _ => theme.warning,
            }
        };
        let state_label = if unapplied > 0 && state == Saving::Saved {
            format!("{unapplied} unapplied")
        } else {
            state.label().to_string()
        };
        let tip = match &state {
            Saving::Failed(e) => format!("The mod could not be saved: {e}"),
            _ if unapplied_movies > 0 && unapplied_particles > 0 => format!("{} ({}): unapplied UI movie and particle curve edits — click Apply before building", p.name, state.label()),
            _ if unapplied_movies > 0 => format!("{} ({}): {unapplied_movies} UI movie{} unapplied stage edits — click Apply in the movie editor or Mod view", p.name, state.label(), if unapplied_movies == 1 { " has" } else { "s have" }),
            _ if unapplied_particles > 0 => format!("{} ({}): unapplied particle curve edits — click Apply in the particle editor or Mod view", p.name, state.label()),
            _ => format!("{} ({}): every change is kept in {}", p.name, state.label(), file_name(&p.file)),
        };
        Some(
            h_flex()
                .gap_1()
                .child(div().size(px(7.0)).rounded_full().bg(color))
                .child(
                    Button::new("project")
                        .small()
                        .ghost()
                        .icon(Icon::new(IconName::Briefcase))
                        .label(format!("{} · {state_label}", p.name))
                        .tooltip(tip)
                        .dropdown_menu(move |menu, _, _| {
                            menu.item(item("Save now", |s, _, cx| s.save_project(cx)))
                                .item(item("Build the mod…", |s, w, cx| s.set_view(super::View::Mod, w, cx)))
                                .item(item("Show the mod's folder", |s, _, cx| {
                                    if let Some(p) = &s.project {
                                        cx.reveal_path(Path::new(&p.file));
                                    }
                                }))
                                .item(item("Open mod-files/ (scripts and extra files)", |s, _, cx| {
                                    if let Some(p) = &s.project {
                                        cx.open_with_system(&p.mod_files());
                                    }
                                }))
                                .separator()
                                .item(item("Discard all changes…", |s, w, cx| s.run(super::Cmd::DiscardAll, w, cx)))
                                .item(item("Patch the game files directly (advanced)…", |s, w, cx| s.run(super::Cmd::WriteToGame, w, cx)))
                                .separator()
                                .item(item("Close the mod…", |s, w, cx| s.close_project(w, cx)))
                        }),
                )
                .into_any_element(),
        )
    }
}
