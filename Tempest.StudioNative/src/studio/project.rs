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

use gpui::{div, prelude::*, px, App, Context, Entity, PathPromptOptions, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
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
            Saving::Pending => "unsaved",
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
                        this.toast(Tone::Error, format!("Could not save the project: {e}"), cx);
                        this.project_saving = Saving::Failed(e);
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
        let name = input(window, cx, "", "My mod");
        let id = input(window, cx, "", "made from the name");
        let author = input(window, cx, "", "your name");
        let version = input(window, cx, "1.0.0", "1.0.0");
        let base = documents().join("Tempest Mods");
        let location = input(window, cx, &base.to_string_lossy(), "folder the project folder goes in");
        let game = self.dir.clone().unwrap_or_default();
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
            let row = |label: &'static str, i: &Entity<InputState>| h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label)).child(div().flex_1().child(Input::new(i).small()));
            let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
            let mod_id = {
                let typed = text(&id, cx);
                if typed.is_empty() { id_of(&text(&name, cx)) } else { typed }
            };
            let folder = Path::new(&text(&location, cx)).join(if mod_id.is_empty() { "<id>" } else { &mod_id }).to_string_lossy().to_string();
            let ok = !mod_id.is_empty();
            let (me, n2, i2, a2, v2, l2) = (me.clone(), name.clone(), id.clone(), author.clone(), version.clone(), location.clone());
            let l3 = location.clone();
            dialog
                .title("New mod project")
                .w(px(620.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child("A project keeps every edit you make while it is open in its own file, saved as you go; the game folder is left untouched until you choose to write into it. Build makes the mod from it."))
                        .child(row("Name", &name))
                        .child(row("Id", &id))
                        .child(row("Author", &author))
                        .child(row("Version", &version))
                        .child(h_flex().gap_2().child(div().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child("Location")).child(div().flex_1().child(Input::new(&location).small())).child(Button::new("np-browse").small().ghost().label("Choose…").on_click(move |_, window, cx| {
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
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("Creates {folder} with {mod_id}.twrk, mod-files/ and builds/. Game folder: {}.", super::folder_label(&game)))),
                )
                .footer(dialogs::footer("Create the project", false, ok, move |window, cx| {
                    let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
                    let name = text(&n2, cx);
                    let typed = text(&i2, cx);
                    let id = if typed.is_empty() { id_of(&name) } else { typed };
                    let p = Project { file: String::new(), name: if name.is_empty() { id.clone() } else { name }, id, author: text(&a2, cx), version: text(&v2, cx), files: Vec::new(), cooker: serde_json::Value::Null };
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
            self.toast(Tone::Error, format!("Could not create {}: {e}", dir.display()), cx);
            return;
        }
        p.file = file.to_string_lossy().to_string();
        let start = move |this: &mut Studio, cx: &mut Context<Studio>| {
            let state = json!({ "project": p });
            this.adopt_project(&p.file, &state);
            this.save_project(cx);
            this.toast(Tone::Success, format!("Project {} created: your edits are kept in it", p.name), cx);
        };
        // Edits made before the project are not part of it unless the user says so.
        if self.unsaved_total(cx) > 0 {
            let me = cx.entity().downgrade();
            let start = std::rc::Rc::new(std::cell::RefCell::new(Some(start)));
            dialogs::confirm(window, cx, "Take the unsaved edits into the project?", "Your current unsaved edits become part of the new project. To start from a clean game folder instead, discard them first (Save ▸ Discard).", "Take them in", false, move |_, cx| {
                if let Some(start) = start.borrow_mut().take() {
                    let _ = me.update(cx, |this, cx| start(this, cx));
                }
            });
        } else {
            start(self, cx);
        }
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

    /// Close the project: ask when it has changes not written yet.
    pub(super) fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.project.clone() else { return };
        let me = cx.entity().downgrade();
        let pending = self.project_saving != Saving::Saved;
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
                .child(div().text_sm().text_color(theme.muted_foreground).child("Its edits are unloaded from the Studio (they stay in the project file and come back when you open it), so nothing of it ends up in the game folder by mistake."))
                .footer(footer)
        });
    }

    /// Save (or not), unload every edit, forget the project, then `then`.
    pub(super) fn close_project_now(&mut self, save: bool, then: AfterClose, cx: &mut Context<Self>) {
        let mut edited: Vec<String> = self.dirty.iter().cloned().collect();
        edited.extend(self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default());
        if let Some(project) = &mut self.project {
            project.note_files(edited);
        }
        let Some(p) = self.project.clone() else { return };
        let save_call = save.then(|| self.backend.call("twrk.save", json!({ "out": p.file, "mode": "linked", "state": self.twrk_state() })));
        let packages: Vec<String> = self.dirty.iter().cloned().collect();
        let docs = self.assembly.is_some() || self.applied_docs;
        let backend = self.backend.clone();
        self.set_status(format!("Closing {}…", p.name), cx);
        cx.spawn(async move |this, cx| {
            if let Some(call) = save_call {
                if let Err(e) = call.await {
                    let _ = this.update(cx, |this, cx| this.toast(Tone::Error, format!("Could not save the project, so it stays open: {e}"), cx));
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

    pub(super) fn unsaved_total(&self, cx: &App) -> usize {
        self.dirty.len() + self.assembly.as_ref().map(|a| a.read(cx).unsaved_count()).unwrap_or(0)
    }

    /// The header's project chip: name, save state, and its menu.
    pub(super) fn render_project_chip(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui_component::menu::{DropdownMenu, PopupMenuItem};
        use gpui_component::Icon;
        use gpui_kit_assets::IconName;
        let p = self.project.clone()?;
        let theme = cx.theme().clone();
        let state = self.project_saving.clone();
        let color = match state {
            Saving::Saved => theme.success,
            Saving::Failed(_) => theme.danger,
            _ => theme.warning,
        };
        let me = cx.entity().downgrade();
        let tip = match &state {
            Saving::Failed(e) => format!("The project could not be saved: {e}"),
            _ => format!("Mod project {} ({}): every edit is kept in {}", p.name, state.label(), file_name(&p.file)),
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
                        .label(format!("{} · {}", p.name, state.label()))
                        .tooltip(tip)
                        .dropdown_menu(move |menu, _, _| {
                            let item = |label: &str, f: fn(&mut Studio, &mut Window, &mut Context<Studio>)| {
                                let me = me.clone();
                                PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                                    let _ = me.update(cx, |this, cx| f(this, window, cx));
                                })
                            };
                            menu.item(item("Save the project now", |s, _, cx| s.save_project(cx)))
                                .item(item("Build the mod…", |s, w, cx| s.set_view(super::View::Mod, w, cx)))
                                .item(item("Show the project folder", |s, _, cx| {
                                    if let Some(p) = &s.project {
                                        cx.reveal_path(Path::new(&p.file));
                                    }
                                }))
                                .item(item("Open mod-files/", |s, _, cx| {
                                    if let Some(p) = &s.project {
                                        cx.open_with_system(&p.mod_files());
                                    }
                                }))
                                .separator()
                                .item(item("Write the edits into the game folder…", |s, w, cx| s.run(super::Cmd::WriteToGame, w, cx)))
                                .separator()
                                .item(item("Close the project…", |s, w, cx| s.close_project(w, cx)))
                        }),
                )
                .into_any_element(),
        )
    }
}
