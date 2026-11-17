//! Tools of the command palette that are not a view of their own: open a
//! package file from anywhere, rewrite a package compressed or not, export a
//! script package's class sources, prepare a mod folder from chosen files
//! (with their recorded bases), discard every unsaved edit, and the
//! package's import and name tables.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, Entity, PathPromptOptions, SharedString, Subscription, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use serde_json::{json, Value as J};

use super::{dialogs, file_name, prefs, Studio, Tone};
use crate::backend::Backend;
use crate::model;

fn documents() -> std::path::PathBuf {
    std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

impl Studio {
    /// Open a package file from anywhere: one of the folder is shown, another joins the list.
    pub(super) fn pick_package_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut p))) = paths.await else { return };
            let Some(file) = p.pop() else { return };
            let _ = this.update(cx, |this, cx| this.open_package_file(file.to_string_lossy().to_string(), cx));
        })
        .detach();
    }

    fn open_package_file(&mut self, path: String, cx: &mut Context<Self>) {
        let same = |a: &str, b: &str| std::path::Path::new(a) == std::path::Path::new(b);
        if self.dir.is_none() || path.to_ascii_lowercase().ends_with(".twrk") {
            // No folder yet: the file's folder becomes it.
            self.open_path(path, cx);
            return;
        }
        if let Some(p) = self.packages.iter().find(|p| same(&p.path, &path)).map(|p| p.path.clone()) {
            self.view = super::View::Packages;
            self.user_open_package(p, cx);
            return;
        }
        // Outside the game folder: listed with the folder's packages while the window lives.
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.packages.push(model::PackageEntry { name: file_name(&path), path: path.clone(), size, script: false, map: false });
        self.view = super::View::Packages;
        self.refresh_rows(cx);
        self.user_open_package(path, cx);
    }

    /// Write a copy of the open package LZO-compressed (and obscured) or uncompressed.
    pub(super) fn rewrite_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()) else { return };
        let (compress, obscure) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(true)));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (c1, c2, o1, o2) = (compress.clone(), compress.clone(), obscure.clone(), obscure.clone());
            let (me, path) = (me.clone(), path.clone());
            let (cz, ob) = (compress.clone(), obscure.clone());
            dialog
                .title(format!("Rewrite {}", file_name(&path)))
                .w(px(480.0))
                .child(
                    v_flex()
                        .gap_3()
                        .child(div().text_sm().text_color(theme.muted_foreground).child("Writes a copy of the file on disk (your changes are not included) in the chosen storage. The game loads both forms; Build mod ships packages compressed and obscured."))
                        .child(Checkbox::new("rw-compress").label("LZO-compress in cooker-style chunks").checked(c1.get()).on_click(move |v, window, _| {
                            c2.set(*v);
                            window.refresh();
                        }))
                        .child(Checkbox::new("rw-obscure").label("XOR-obscure the chunks (Hi-Rez flag)").checked(o1.get() && c1.get()).disabled(!c1.get()).on_click(move |v, window, _| {
                            o2.set(*v);
                            window.refresh();
                        })),
                )
                .footer(dialogs::footer("Write…", false, true, move |window, cx| {
                    window.close_dialog(cx);
                    let (compress, obscure) = (cz.get(), cz.get() && ob.get());
                    let path = path.clone();
                    let _ = me.update(cx, |this, cx| this.rewrite(path, compress, obscure, cx));
                }))
        });
    }

    fn rewrite(&mut self, path: String, compress: bool, obscure: bool, cx: &mut Context<Self>) {
        let name = file_name(&path);
        let stem = name.strip_suffix(".upk").or_else(|| name.strip_suffix(".UPK")).unwrap_or(&name);
        let target = cx.prompt_for_new_path(&documents(), Some(&format!("{stem}.{}.upk", if compress { "compressed" } else { "decompressed" })));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status("Rewriting…", cx);
                this.backend.call("package.rewrite", json!({ "path": path, "out": out.to_string_lossy(), "compress": compress, "obscure": obscure }))
            }) else {
                return;
            };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.toast(Tone::Success, format!("{}: {}", if compress { "Compressed" } else { "Decompressed" }, r.json["out"].as_str().unwrap_or_default()), cx),
                Err(e) => this.toast(Tone::Error, format!("Rewrite failed: {e}"), cx),
            });
        })
        .detach();
    }

    /// Decompile every class of the open script package into `<folder>/<Package>/Classes/*.uc`.
    pub(super) fn export_sources(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()) else { return };
        let pick = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Export the sources here".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(dir) = dirs.pop() else { return };
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status("Decompiling…", cx);
                this.backend.call("script.export", json!({ "path": path, "outDir": dir.to_string_lossy() }))
            }) else {
                return;
            };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.toast(Tone::Success, format!("Sources exported: {} classes in {}", r.json["classes"], r.json["dir"].as_str().unwrap_or_default()), cx),
                Err(e) => this.toast(Tone::Error, format!("Decompile failed: {e}"), cx),
            });
        })
        .detach();
    }

    /// Write the selected object's serialized payload to a file, as it is stored.
    pub(super) fn extract_raw(&mut self, cx: &mut Context<Self>) {
        let (Some(t), Some(i)) = (&self.open, self.selected_export) else { return };
        let (path, name) = (t.path.clone(), t.object_path(i));
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
        let target = cx.prompt_for_new_path(&documents(), Some(&format!("{safe}.bin")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let Ok(call) = this.update(cx, |this, _| this.backend.call("object.extract", json!({ "path": path, "index": i, "out": out.to_string_lossy() }))) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.toast(Tone::Success, format!("Extracted {} to {}", model::human_size(r.json["bytes"].as_u64().unwrap_or(0)), out.display()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not extract: {e}"), cx),
            });
        })
        .detach();
    }

    /// Drop every unsaved edit of the window (packages and assembly files), after asking.
    pub(super) fn discard_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut files: Vec<String> = self.dirty.iter().cloned().collect();
        files.sort();
        let asm = self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default();
        let n = files.len() + asm.len();
        if n == 0 {
            self.toast(Tone::Info, "Nothing is edited", cx);
            return;
        }
        let mut names: Vec<String> = files.iter().chain(asm.iter()).take(12).map(|p| file_name(p)).collect();
        if n > names.len() {
            names.push(format!("… and {} more", n - names.len()));
        }
        let me = cx.entity().downgrade();
        dialogs::confirm(window, cx, format!("Discard the changes to {n} file{}?", if n == 1 { "" } else { "s" }), format!("Every edit is lost and the files are read again from the game folder: {}.", names.join(", ")), "Discard all", true, move |_, cx| {
            let files = files.clone();
            let _ = me.update(cx, |this, cx| {
                for path in files {
                    this.discard_now(path, cx);
                }
                if let Some(a) = this.assembly.clone() {
                    a.update(cx, |a, cx| a.discard_all(cx));
                }
            });
        });
    }

    /// Dark ↔ light (the Settings also offer "like the system").
    pub(super) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().mode.is_dark() { "light" } else { "dark" };
        prefs::theme_mode(next, Some(window), cx);
        self.prefs.theme = next.into();
        self.prefs.save();
        window.refresh();
    }

    /// Show the open project's folder in the file manager.
    pub(super) fn show_project_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.project {
            cx.reveal_path(&p.dir());
        }
    }

    /// Prepare a mod folder from chosen files (the open package to start with).
    pub(super) fn mod_folder_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let files: Vec<String> = self.open.as_ref().map(|t| t.path.clone()).into_iter().collect();
        let (backend, me) = (self.backend.clone(), cx.entity().downgrade());
        let dirty: Vec<String> = self.dirty.iter().cloned().collect();
        let view = cx.new(|cx| ModFolder::new(backend, me, files, dirty, window, cx));
        window.open_dialog(cx, move |dialog, _, _| dialog.title("Prepare a mod folder").w(px(640.0)).child(view.clone()));
    }

    /// The imports and names of a package, filtered (the Package info dialog's lists).
    pub(super) fn package_tables_dialog(&mut self, path: String, names: bool, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let title = format!("{}: {}", file_name(&path), if names { "names" } else { "imports" });
        let view = cx.new(|cx| NameList::new(backend, path, names, window, cx));
        window.open_dialog(cx, move |dialog, _, _| dialog.title(title.clone()).w(px(720.0)).child(view.clone()));
    }
}

/// The "Prepare a mod folder" dialog: `manifest.toml` and the chosen files,
/// each shipped as a delta when its base is recorded, else whole.
pub struct ModFolder {
    backend: Backend,
    studio: WeakEntity<Studio>,
    id: Entity<InputState>,
    name: Entity<InputState>,
    version: Entity<InputState>,
    author: Entity<InputState>,
    out: Entity<InputState>,
    files: Vec<String>,
    unsaved: Vec<String>,
    /// Whether each file's base is recorded, and when (Unix seconds).
    status: HashMap<String, (bool, Option<i64>)>,
    recording: Option<String>,
    /// The folder written and what went into it.
    done: Option<(String, Vec<J>)>,
    _subs: Vec<Subscription>,
}

impl ModFolder {
    fn new(backend: Backend, studio: WeakEntity<Studio>, files: Vec<String>, unsaved: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> ModFolder {
        let input = |ph: &str, value: &str, window: &mut Window, cx: &mut Context<Self>| {
            let (ph, value) = (ph.to_string(), value.to_string());
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(ph);
                s.set_value(value, window, cx);
                s
            })
        };
        let id = input("my-skin-pack", "", window, cx);
        let name = input("My skin pack", "", window, cx);
        let version = input("1.0.0", "1.0.0", window, cx);
        let author = input("", "", window, cx);
        let out = input("output folder", "", window, cx);
        // The Create button follows the fields.
        let subs = [&id, &out].into_iter().map(|i| cx.subscribe(i, |_, _, e: &InputEvent, cx| if matches!(e, InputEvent::Change) { cx.notify() })).collect();
        let mut m = ModFolder { backend, studio, id, name, version, author, out, files: Vec::new(), unsaved, status: HashMap::new(), recording: None, done: None, _subs: subs };
        m.add(files, cx);
        m
    }

    /// List more files and look up how each ships.
    fn add(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        for f in files {
            if self.files.contains(&f) {
                continue;
            }
            self.files.push(f.clone());
            let call = self.backend.call("delta.status", json!({ "path": f }));
            cx.spawn(async move |this, cx| {
                let r = call.await;
                let _ = this.update(cx, |this, cx| {
                    let s = r.map(|r| (r.json["recorded"].as_bool().unwrap_or(false), r.json["when"].as_i64())).unwrap_or((false, None));
                    this.status.insert(f, s);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = pick.await else { return };
            let _ = this.update(cx, |this, cx| this.add(paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect(), cx));
        })
        .detach();
    }

    fn pick_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Use this folder".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(dir) = dirs.pop() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                this.out.update(cx, |s, cx| s.set_value(dir.to_string_lossy().to_string(), window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// Take the file as it is now as its base, so later edits ship as a delta.
    fn record_base(&mut self, file: String, cx: &mut Context<Self>) {
        self.recording = Some(file.clone());
        let call = self.backend.call("delta.record", json!({ "path": file }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.recording = None;
                let studio = this.studio.clone();
                let (tone, text) = match r {
                    Ok(r) => {
                        this.status.insert(file.clone(), (r.json["recorded"].as_bool().unwrap_or(true), r.json["when"].as_i64()));
                        (Tone::Success, format!("{}: this version is now the base", file_name(&file)))
                    }
                    Err(e) => (Tone::Error, format!("Could not record the base: {e}")),
                };
                cx.defer(move |cx| {
                    let _ = studio.update(cx, |s, cx| s.toast(tone, text, cx));
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn ready(&self, cx: &Context<Self>) -> bool {
        !self.id.read(cx).value().trim().is_empty() && !self.out.read(cx).value().trim().is_empty() && !self.files.is_empty()
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let text = |i: &Entity<InputState>| i.read(cx).value().trim().to_string();
        let mut params = json!({ "dir": text(&self.out), "id": text(&self.id), "files": self.files });
        for (key, input) in [("name", &self.name), ("version", &self.version), ("author", &self.author)] {
            let v = text(input);
            if !v.is_empty() {
                params[key] = json!(v);
            }
        }
        let call = self.backend.call("mod.dir", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                let studio = this.studio.clone();
                let (tone, text) = match r {
                    Ok(r) => {
                        let entries: Vec<J> = r.json["entries"].as_array().cloned().unwrap_or_default();
                        let deltas = entries.iter().filter(|e| e["mode"] == "delta").count();
                        let n = r.json["files"].as_array().map(|f| f.len()).unwrap_or(0);
                        this.done = Some((r.json["dir"].as_str().unwrap_or_default().to_string(), entries));
                        (Tone::Success, format!("Mod folder ready: {n} files written{}", if deltas > 0 { format!(", {deltas} as deltas") } else { String::new() }))
                    }
                    Err(e) => (Tone::Error, format!("Could not create the mod folder: {e}")),
                };
                cx.defer(move |cx| {
                    let _ = studio.update(cx, |s, cx| s.toast(tone, text, cx));
                });
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ModFolder {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let field = |label: &'static str, input: &Entity<InputState>| v_flex().flex_1().gap_1().child(div().text_xs().text_color(muted).child(label)).child(Input::new(input).small());
        let badge = |text: &'static str, color: gpui::Hsla| div().flex_none().px_1().rounded_sm().text_xs().bg(color.opacity(0.18)).text_color(color).child(text);
        let mut list = v_flex().id("mf-files").max_h(px(160.0)).overflow_y_scroll().p_1().rounded_md().bg(theme.secondary).font_family(mono.clone()).text_xs();
        if self.files.is_empty() {
            list = list.child(div().px_1().py_0p5().text_color(muted).child("no files yet"));
        }
        for (k, f) in self.files.iter().enumerate() {
            let mut row = h_flex().gap_2().px_1().py_0p5().child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(file_name(f)));
            if self.unsaved.contains(f) {
                row = row.child(badge("edited", theme.warning));
            }
            match self.status.get(f) {
                Some((true, _)) => row = row.child(badge("delta", theme.success)),
                Some((false, _)) => {
                    let (file, busy) = (f.clone(), self.recording.as_deref() == Some(f.as_str()));
                    row = row.child(badge("whole file", muted)).child(
                        Button::new(("mf-record", k))
                            .xsmall()
                            .ghost()
                            .label(if busy { "recording…" } else { "record base" })
                            .disabled(self.recording.is_some())
                            .tooltip("Take this file as it is now as the base, so later edits ship as a delta (only if it is the game's original)")
                            .on_click(cx.listener(move |this, _, _, cx| this.record_base(file.clone(), cx))),
                    );
                }
                None => {}
            }
            let file = f.clone();
            row = row.child(Button::new(("mf-remove", k)).xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Trash)).tooltip("Remove from the list").on_click(cx.listener(move |this, _, _, cx| {
                this.files.retain(|x| x != &file);
                cx.notify();
            })));
            list = list.child(row);
        }
        let mut body = v_flex()
            .gap_3()
            .child(div().text_xs().text_color(muted).child("Creates manifest.toml and the chosen files, ready for `tempest mod pack` (or the Mod view), which packages and signs it. A package or .dat file saved in place from the Studio ships as a delta: only the objects or rows that changed, applied over the game's file by the launcher, so mods editing different parts of one file work together. A file without a recorded base is copied whole. Save your edited packages first."))
            .child(h_flex().gap_2().child(field("Mod id", &self.id)).child(field("Display name", &self.name)))
            .child(h_flex().gap_2().child(field("Version", &self.version)).child(field("Author", &self.author)))
            .child(v_flex().gap_1().child(h_flex().child(div().flex_1().min_w_0().text_xs().text_color(muted).child("Files")).child(Button::new("mf-add").xsmall().ghost().label("Add…").on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))))).child(list))
            .child(h_flex().gap_1().child(div().flex_1().min_w_0().child(Input::new(&self.out).small())).child(Button::new("mf-browse").small().label("Browse…").on_click(cx.listener(|this, _, window, cx| this.pick_out(window, cx)))));
        if let Some((dir, entries)) = &self.done {
            let open = dir.clone();
            let kb = |n: u64| if n >= 1 << 20 { format!("{:.1} MB", n as f64 / (1 << 20) as f64) } else { format!("{:.1} KB", n as f64 / 1024.0) };
            let mut out = v_flex().p_2().rounded_md().bg(theme.secondary).font_family(mono).text_xs();
            for e in entries {
                let what = match e["mode"].as_str() {
                    Some("delta") => format!("{} changed objects, {}", e["ops"], kb(e["bytes"].as_u64().unwrap_or(0))),
                    Some("copy") => format!("whole file, {}", kb(e["bytes"].as_u64().unwrap_or(0))),
                    _ => "unchanged, skipped".to_string(),
                };
                out = out.child(div().child(format!("{}: {what}", e["file"].as_str().unwrap_or_default())));
                for w in e["warnings"].as_array().into_iter().flatten() {
                    out = out.child(div().text_color(theme.warning).child(w.as_str().unwrap_or_default().to_string()));
                }
            }
            body = body
                .child(h_flex().gap_2().p_2().rounded_md().bg(theme.success.opacity(0.15)).text_xs().child(div().flex_1().min_w_0().child(format!("Ready in {dir}. Package and sign it in the Mod view, or with `just mod-pack \"{dir}\" out.tempest`."))).child(Button::new("mf-open").xsmall().label("Open").on_click(move |_, _, cx| cx.open_with_system(std::path::Path::new(&open)))))
                .child(out);
        }
        let ready = self.ready(cx);
        body.child(
            h_flex()
                .justify_end()
                .gap_2()
                .child(Button::new("mf-close").ghost().label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                .child(Button::new("mf-create").primary().label("Create").disabled(!ready).when(!ready, |b| b.tooltip("Give it an id and an output folder, and add at least one package")).on_click(cx.listener(|this, _, _, cx| this.create(cx)))),
        )
    }
}

/// A package's imports or its name table, filtered as typed (only the rows in view are laid out).
pub struct NameList {
    /// (index, text, detail)
    rows: Vec<(usize, String, String)>,
    shown: Vec<usize>,
    filter: Entity<InputState>,
    loading: bool,
    error: Option<String>,
    names: bool,
    _sub: Subscription,
}

impl NameList {
    fn new(backend: Backend, path: String, names: bool, window: &mut Window, cx: &mut Context<Self>) -> NameList {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter…"));
        let sub = cx.subscribe(&filter, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.apply_filter(cx);
            }
        });
        let call = if names { backend.call("package.names", json!({ "path": path })) } else { backend.call("package.tables", json!({ "path": path })) };
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match r {
                    Ok(r) if names => this.rows = r.json.as_array().into_iter().flatten().enumerate().map(|(i, n)| (i, n.as_str().unwrap_or_default().to_string(), String::new())).collect(),
                    Ok(r) => {
                        this.rows = r.json["imports"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .enumerate()
                            .map(|(i, imp)| (i, imp["path"].as_str().or(imp["name"].as_str()).unwrap_or_default().to_string(), imp["class"].as_str().unwrap_or_default().to_string()))
                            .collect()
                    }
                    Err(e) => this.error = Some(e),
                }
                this.apply_filter(cx);
            });
        })
        .detach();
        NameList { rows: Vec::new(), shown: Vec::new(), filter, loading: true, error: None, names, _sub: sub }
    }

    fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let f = self.filter.read(cx).value().trim().to_ascii_lowercase();
        self.shown = (0..self.rows.len()).filter(|&k| f.is_empty() || self.rows[k].1.to_ascii_lowercase().contains(&f) || self.rows[k].2.to_ascii_lowercase().contains(&f)).collect();
        cx.notify();
    }

    fn render_rows(&mut self, range: std::ops::Range<usize>, _: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        range
            .map(|k| {
                let (i, text, detail) = &self.rows[self.shown[k]];
                h_flex()
                    .h(px(22.0))
                    .gap_3()
                    .px_2()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .child(div().w(px(56.0)).flex_none().text_right().text_color(theme.muted_foreground).child(i.to_string()))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(text.clone()))
                    .child(div().flex_none().text_color(theme.muted_foreground).child(detail.clone()))
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for NameList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let what = if self.names { "names" } else { "imports" };
        let count = if self.loading { "Reading…".to_string() } else if let Some(e) = &self.error { e.clone() } else { format!("{} of {} {what}", self.shown.len(), self.rows.len()) };
        v_flex()
            .gap_2()
            .child(h_flex().gap_2().child(div().flex_1().min_w_0().child(Input::new(&self.filter).small().cleanable(true))).child(div().text_xs().text_color(theme.muted_foreground).child(SharedString::from(count))))
            .child(div().h(px(420.0)).rounded_md().border_1().border_color(theme.border).child(uniform_list("name-rows", self.shown.len(), cx.processor(Self::render_rows)).size_full()))
            .child(h_flex().justify_end().child(Button::new("nl-close").primary().label("Close").on_click(|_, window, cx| window.close_dialog(cx))))
    }
}
