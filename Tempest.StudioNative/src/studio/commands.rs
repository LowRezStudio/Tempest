//! Every action of the Studio as a command: a title, a group, its keys and
//! when it can run. The key bindings, the command palette (`>`), the menus
//! and the shortcut list (F1) all read this table, so they never disagree.

use gpui::{div, px, App, Context, KeyBinding, Stateful, Div, Window};
use gpui_component::{h_flex, v_flex, ActiveTheme, WindowExt};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    OpenFolder,
    NewWindow,
    HideSide,
    HideSecond,
    Create,
    Save,
    SaveAll,
    SaveAs,
    Discard,
    Undo,
    Back,
    Forward,
    Commands,
    GoToPackage,
    GoToObject,
    SearchAll,
    NewTab,
    CloseTab,
    ShowPackages,
    ShowAssembly,
    ShowMod,
    ShowMap,
    MapAddActor,
    MapCollision,
    MapBakeLighting,
    MapNewMatinee,
    MapOpenMatinee,
    MapMatineeKey,
    NewPackage,
    AddObject,
    AddGroup,
    Rename,
    Duplicate,
    CopyTo,
    Delete,
    ExportObject,
    ReplaceObject,
    ReplaceWith,
    FindReplace,
    BatchImport,
    Verify,
    NewInstance,
    OwnMaterials,
    SaveWorkspace,
    SaveWorkspacePortable,
    OpenWorkspaceFile,
    NewProject,
    OpenProject,
    CloseProject,
    WriteToGame,
    CopyPath,
    Settings,
    Shortcuts,
}

pub struct Info {
    pub title: &'static str,
    pub group: &'static str,
    /// macOS keys (`⌘` becomes Ctrl elsewhere).
    pub keys: &'static str,
    /// What it does, for the palette.
    pub detail: &'static str,
}

impl Cmd {
    pub const ALL: &'static [Cmd] = &[
        Cmd::OpenFolder,
        Cmd::NewWindow,
        Cmd::HideSide,
        Cmd::HideSecond,
        Cmd::Create,
        Cmd::Save,
        Cmd::SaveAll,
        Cmd::SaveAs,
        Cmd::Discard,
        Cmd::Undo,
        Cmd::Back,
        Cmd::Forward,
        Cmd::Commands,
        Cmd::GoToPackage,
        Cmd::GoToObject,
        Cmd::SearchAll,
        Cmd::NewTab,
        Cmd::CloseTab,
        Cmd::ShowPackages,
        Cmd::ShowAssembly,
        Cmd::ShowMod,
        Cmd::ShowMap,
        Cmd::MapAddActor,
        Cmd::MapCollision,
        Cmd::MapBakeLighting,
        Cmd::MapNewMatinee,
        Cmd::MapOpenMatinee,
        Cmd::MapMatineeKey,
        Cmd::NewPackage,
        Cmd::AddObject,
        Cmd::AddGroup,
        Cmd::Rename,
        Cmd::Duplicate,
        Cmd::CopyTo,
        Cmd::Delete,
        Cmd::ExportObject,
        Cmd::ReplaceObject,
        Cmd::ReplaceWith,
        Cmd::FindReplace,
        Cmd::BatchImport,
        Cmd::Verify,
        Cmd::NewInstance,
        Cmd::OwnMaterials,
        Cmd::SaveWorkspace,
        Cmd::SaveWorkspacePortable,
        Cmd::OpenWorkspaceFile,
        Cmd::NewProject,
        Cmd::OpenProject,
        Cmd::CloseProject,
        Cmd::WriteToGame,
        Cmd::CopyPath,
        Cmd::Settings,
        Cmd::Shortcuts,
    ];

    pub fn info(self) -> Info {
        let i = |title, group, keys, detail| Info { title, group, keys, detail };
        match self {
            Cmd::OpenFolder => i("Open game folder…", "Workspace", "", "the game's ChaosGame/CookedPCConsole folder"),
            Cmd::HideSide => i("Show or hide the side pane", "View", "⌘B", "the package list, the assembly files, the sound banks or the map's levels; over the view in a narrow window"),
            Cmd::HideSecond => i("Show or hide the second pane", "View", "⌘J", "the object list, the row pane, the events or the map's inspector"),
            Cmd::NewWindow => i("New window", "Workspace", "⌘⇧N", "another window, for another game folder or package"),
            Cmd::Create => i("Create…", "Edit", "⌘N", "a texture, mesh, material, sound, package, skin, ability, item or champion, step by step"),
            Cmd::Save => i("Save", "Workspace", "⌘S", "the project when one is open, else the open package or assembly files in place (asks first)"),
            Cmd::SaveAll => i("Save every unsaved file", "Workspace", "", "every package with edits, in place (asks first)"),
            Cmd::SaveAs => i("Export a copy of the package…", "Workspace", "", "write it elsewhere; the game folder is untouched and the edits stay unsaved"),
            Cmd::Discard => i("Discard the package's changes…", "Workspace", "", "reload it from the game folder"),
            Cmd::Undo => i("Undo", "Edit", "⌘Z", "take back the last edit"),
            Cmd::Back => i("Back", "Go", "⌥←", ""),
            Cmd::Forward => i("Forward", "Go", "⌥→", ""),
            Cmd::Commands => i("Commands…", "Go", "⌘K", ""),
            Cmd::GoToPackage => i("Go to a package…", "Go", "⌘P", ""),
            Cmd::GoToObject => i("Go to an object of this package…", "Go", "⌘⇧O", ""),
            Cmd::SearchAll => i("Search every package…", "Go", "⌘⇧F", "objects by name, in every package of the folder"),
            Cmd::NewTab => i("New tab", "Go", "⌘T", "a copy of this tab"),
            Cmd::CloseTab => i("Close the tab", "Go", "⌘W", ""),
            Cmd::ShowPackages => i("Show packages", "View", "⌘1", ""),
            Cmd::ShowAssembly => i("Show the assembly tables", "View", "⌘2", "assembly.dat and the language files"),
            Cmd::ShowMod => i("Make a mod", "View", "⌘3", "the mod cooker: what your changes become"),
            Cmd::ShowMap => i("Show the maps", "View", "⌘5", "a map and its sublevels in 3D, Kismet and Matinee"),
            Cmd::MapAddActor => i("Add an actor to the map…", "Map", "⇧A", "a copy of an actor of any level, a new one of a class, or a mesh, where the view looks"),
            Cmd::MapCollision => i("Show collision in the map", "Map", "", "volumes' convex hulls and meshes' simple or complex collision, by what they block"),
            Cmd::MapBakeLighting => i("Bake lighting…", "Map", "", "static light maps for the meshes added or moved, the selection or whole levels"),
            Cmd::MapNewMatinee => i("New Matinee", "Map", "", "in the selected actor's level, moving it (or empty when nothing is selected)"),
            Cmd::MapOpenMatinee => i("Open the level's Matinees", "Map", "", "the first Matinee of the selected actor's level (the timeline lists the others)"),
            Cmd::MapMatineeKey => i("Add a Matinee key at the playhead", "Map", "K", "on the selected track; a move key takes where its actor is"),
            Cmd::NewPackage => i("New package…", "Package", "", "an empty package with a game package's settings"),
            Cmd::AddObject => i("Add to the package…", "Package", "", "an object from a file, a copy from another package, or a group"),
            Cmd::AddGroup => i("Add a group…", "Package", "", "a folder for objects inside the package"),
            Cmd::Rename => i("Rename", "Object", "F2", ""),
            Cmd::Duplicate => i("Duplicate", "Object", "⌘D", "a copy next to it"),
            Cmd::CopyTo => i("Copy to another package…", "Object", "", "with what it uses, references remapped"),
            Cmd::Delete => i("Delete…", "Object", "⌫", "with its sub-objects; references to it are cleared"),
            Cmd::ExportObject => i("Export to a file…", "Object", "", "PNG for textures, glTF for meshes, Ogg for sounds, else the raw payload"),
            Cmd::ReplaceObject => i("Replace from a file…", "Object", "", "a texture, mesh, sound, or a raw payload"),
            Cmd::ReplaceWith => i("Replace with an object of another package…", "Object", "", "it becomes a copy of it, keeping its name"),
            Cmd::FindReplace => i("Find and replace in the package…", "Package", "", "strings, names and references"),
            Cmd::BatchImport => i("Batch import a folder…", "Package", "", "<package>.<object>.<ext> files into the packages they name"),
            Cmd::Verify => i("Check the package", "Package", "", "rewrite every object and compare (round trip)"),
            Cmd::NewInstance => i("New material instance…", "Material", "", "a MaterialInstanceConstant of the selected material, with no overrides"),
            Cmd::OwnMaterials => i("Give the mesh its own materials", "Material", "", "every slot gets a material of this mesh only, in one group"),
            Cmd::SaveWorkspace => i("Save a workspace file…", "Workspace", "⌘⌥S", "every unsaved edit and where you are, for later (the game folder is untouched)"),
            Cmd::SaveWorkspacePortable => i("Save a portable workspace file…", "Workspace", "", "the changed files whole: opens on another machine or game copy"),
            Cmd::OpenWorkspaceFile => i("Open a workspace file…", "Workspace", "⌘⌥O", "puts its edits back, unsaved"),
            Cmd::NewProject => i("New mod project…", "Project", "", "a folder that keeps every edit of your mod, saved as you go"),
            Cmd::OpenProject => i("Open a mod project…", "Project", "", "its edits come back, the game folder untouched"),
            Cmd::CloseProject => i("Close the project…", "Project", "", "its edits are unloaded until you open it again"),
            Cmd::WriteToGame => i("Write into the game folder", "Workspace", "", "the open package or assembly files, in place (asks first)"),
            Cmd::CopyPath => i("Copy the object path", "Object", "⌘⇧C", "Package.Group.Name, as the search and other packages name it"),
            Cmd::Settings => i("Settings…", "Help", "⌘,", "theme, the save warning, reopening where you were"),
            Cmd::Shortcuts => i("Keyboard shortcuts", "Help", "F1", ""),
        }
    }

    /// The driver's name for it (`run save-all`).
    pub fn id(self) -> String {
        let s = format!("{self:?}");
        let mut out = String::new();
        for (k, c) in s.chars().enumerate() {
            if c.is_uppercase() && k > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    pub fn enabled(self, s: &Studio) -> bool {
        let object = s.selected_export.is_some() && s.open.is_some();
        let class = s.selection_class();
        match self {
            // In the map view ⌘D and Delete act on the selected actor.
            Cmd::Duplicate | Cmd::Delete if s.view == View::Map => s.map.is_some(),
            Cmd::MapAddActor | Cmd::MapCollision | Cmd::MapBakeLighting | Cmd::MapNewMatinee | Cmd::MapOpenMatinee | Cmd::MapMatineeKey => s.view == View::Map && s.map.is_some(),
            Cmd::ShowMap => s.dir.is_some(),
            Cmd::Rename | Cmd::CopyPath | Cmd::Duplicate | Cmd::CopyTo | Cmd::Delete | Cmd::ExportObject | Cmd::ReplaceObject | Cmd::ReplaceWith => object,
            Cmd::AddObject | Cmd::AddGroup | Cmd::FindReplace | Cmd::Verify | Cmd::SaveAs | Cmd::GoToObject => s.open.is_some(),
            Cmd::Discard => s.open.as_ref().is_some_and(|t| s.dirty.contains(&t.path)),
            Cmd::SaveAll => !s.dirty.is_empty(),
            Cmd::NewInstance => class.as_deref().is_some_and(material::is_material),
            Cmd::OwnMaterials => class.as_deref().is_some_and(mesh::is_mesh),
            Cmd::SaveWorkspace | Cmd::SaveWorkspacePortable | Cmd::NewProject => s.dir.is_some(),
            Cmd::CloseProject => s.project.is_some(),
            Cmd::Create | Cmd::NewPackage | Cmd::BatchImport | Cmd::GoToPackage | Cmd::SearchAll => s.dir.is_some(),
            _ => true,
        }
    }
}

/// `⌘⇧O` as this platform writes it.
pub fn key_hint(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        keys.to_string()
    } else {
        keys.replace('⌘', "Ctrl+").replace('⇧', "Shift+").replace('⌥', "Alt+").replace('⌫', "Delete")
    }
}

impl Studio {
    pub(crate) fn selection_class(&self) -> Option<String> {
        let t = self.open.as_ref()?;
        Some(t.exports.get(self.selected_export?)?.class.clone())
    }

    pub fn run(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        if !cmd.enabled(self) {
            let why = match cmd {
                Cmd::NewInstance => "Select a material or material instance first",
                Cmd::OwnMaterials => "Select a static or skeletal mesh first",
                Cmd::Discard => "This package has no unsaved changes",
                Cmd::CloseProject => "No project is open",
                Cmd::SaveAll => "Nothing is unsaved",
                _ if self.dir.is_none() => "Open a game folder first",
                _ if self.open.is_none() => "Open a package first",
                _ => "Select an object first",
            };
            self.toast(Tone::Info, why, cx);
            return;
        }
        match cmd {
            Cmd::OpenFolder => self.pick_workspace(cx),
            Cmd::Create => self.open_wizard(None, None, window, cx),
            Cmd::HideSide => self.toggle_pane(false, cx),
            Cmd::HideSecond => self.toggle_pane(true, cx),
            Cmd::NewWindow => {
                cx.defer(|cx| {
                    super::open_window(None, cx);
                });
            }
            Cmd::Save => self.save(&Save, window, cx),
            Cmd::SaveAll => {
                let mut all: Vec<String> = self.dirty.iter().cloned().collect();
                all.sort();
                self.save_packages(all, window, cx);
            }
            Cmd::SaveAs => {
                if let Some(p) = self.open.as_ref().map(|t| t.path.clone()) {
                    self.save_copy(p, cx);
                }
            }
            Cmd::Discard => {
                if let Some(p) = self.open.as_ref().map(|t| t.path.clone()) {
                    self.discard(p, window, cx);
                }
            }
            Cmd::Undo => self.undo(&Undo, window, cx),
            Cmd::Back => self.back(&Back, window, cx),
            Cmd::Forward => self.forward(&Forward, window, cx),
            Cmd::Commands => self.open_palette(">", window, cx),
            Cmd::GoToPackage => self.open_palette("", window, cx),
            Cmd::GoToObject => self.open_palette("@", window, cx),
            Cmd::SearchAll => self.open_palette("#", window, cx),
            Cmd::NewTab => self.new_tab(None, cx),
            Cmd::CloseTab => self.close_tab(self.active_tab, cx),
            Cmd::ShowPackages => self.set_view(View::Packages, window, cx),
            Cmd::ShowAssembly => self.set_view(View::Assembly, window, cx),
            Cmd::ShowMod => self.set_view(View::Mod, window, cx),
            Cmd::ShowMap => self.set_view(View::Map, window, cx),
            Cmd::Duplicate if self.view == View::Map => {
                if let Some(m) = self.map.clone() {
                    m.update(cx, |m, cx| m.duplicate(cx));
                }
            }
            Cmd::Delete if self.view == View::Map => {
                if let Some(m) = self.map.clone() {
                    m.update(cx, |m, cx| m.delete_key_or_actor(cx));
                }
            }
            Cmd::MapAddActor | Cmd::MapCollision | Cmd::MapBakeLighting | Cmd::MapNewMatinee | Cmd::MapOpenMatinee | Cmd::MapMatineeKey => {
                if let Some(m) = self.map.clone() {
                    m.update(cx, |m, cx| m.run_command(cmd, window, cx));
                }
            }
            Cmd::NewPackage => self.open_wizard(Some(wizard::Kind::Package), None, window, cx),
            Cmd::AddObject => self.open_wizard(None, Some(false), window, cx),
            Cmd::AddGroup => self.open_wizard(Some(wizard::Kind::Group), None, window, cx),
            Cmd::Rename => self.start_rename(window, cx),
            Cmd::Duplicate => self.duplicate(cx),
            Cmd::CopyTo => self.open_picker(palette::Pick::CopyTo, window, cx),
            Cmd::Delete => self.delete(window, cx),
            Cmd::ExportObject => self.export_selected(window, cx),
            Cmd::ReplaceObject => self.replace_selected(window, cx),
            Cmd::ReplaceWith => self.open_picker(palette::Pick::ReplaceWith, window, cx),
            Cmd::FindReplace => self.find_replace_dialog(window, cx),
            Cmd::BatchImport => self.batch_import(window, cx),
            Cmd::Verify => {
                if let Some(p) = self.open.as_ref().map(|t| t.path.clone()) {
                    self.verify(p, window, cx);
                }
            }
            Cmd::NewInstance => self.new_instance(cx),
            Cmd::OwnMaterials => self.own_materials(cx),
            Cmd::SaveWorkspace => self.save_workspace(false, cx),
            Cmd::SaveWorkspacePortable => self.save_workspace(true, cx),
            Cmd::OpenWorkspaceFile => self.pick_workspace_file(cx),
            Cmd::NewProject => self.new_project_dialog(window, cx),
            Cmd::OpenProject => self.pick_project(cx),
            Cmd::CloseProject => self.close_project(window, cx),
            Cmd::WriteToGame => self.write_to_game(window, cx),
            Cmd::CopyPath => {
                if let (Some(t), Some(i)) = (&self.open, self.selected_export) {
                    let path = t.object_path(i);
                    self.copy_text(path, "object path", cx);
                }
            }
            Cmd::Settings => self.show_settings(window, cx),
            Cmd::Shortcuts => self.show_shortcuts(window, cx),
        }
    }

    pub(super) fn show_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut groups: Vec<(&'static str, Vec<(&'static str, String)>)> = Vec::new();
        for c in Cmd::ALL {
            let info = c.info();
            if info.keys.is_empty() {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == info.group) {
                Some(g) => g.1.push((info.title, key_hint(info.keys))),
                None => groups.push((info.group, vec![(info.title, key_hint(info.keys))])),
            }
        }
        let extra = [
            ("Lists", "↑ ↓ Home End: move · ← →: collapse, expand, parent · ⌘-click or middle-click: new tab · right-click: what can be done"),
            ("Inspector", "click a value to edit it, Enter to apply, Esc to cancel · click a boolean to flip it"),
            ("3D view", "drag: orbit · wheel: zoom · double-click: fit"),
        ];
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let mut body = v_flex().gap_3();
            for (g, items) in &groups {
                let mut col = v_flex().gap_0p5().child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(g.to_uppercase()));
                for (t, k) in items {
                    col = col.child(h_flex().text_sm().child(div().flex_1().child(*t)).child(div().font_family("Menlo").text_xs().px_1().rounded_sm().bg(theme.secondary).child(k.clone())));
                }
                body = body.child(col);
            }
            for (g, text) in extra {
                body = body.child(v_flex().gap_0p5().child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(g.to_uppercase())).child(div().text_sm().child(text)));
            }
            dialog.title("Keyboard shortcuts").w(px(560.0)).child(div().id("shortcuts").max_h(px(560.0)).overflow_y_scroll().child(body))
        });
    }

    /// Dev: a step of the test driver that is not an input event.
    pub fn drive(&mut self, verb: &str, arg: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let arg = arg.trim();
        match verb {
            "folder" => self.open_workspace(arg.to_string(), cx),
            "package" => {
                let p = self.packages.iter().find(|p| p.name.eq_ignore_ascii_case(arg) || file_name(&p.path).eq_ignore_ascii_case(arg)).ok_or_else(|| format!("no package {arg}"))?;
                let path = p.path.clone();
                self.user_open_package(path, cx);
            }
            "select" => {
                let t = self.open.as_ref().ok_or("no package is open")?;
                let i = match arg.parse::<usize>() {
                    Ok(i) => i,
                    Err(_) => (0..t.exports.len()).find(|&i| t.exports[i].name.eq_ignore_ascii_case(arg)).ok_or_else(|| format!("no export {arg}"))?,
                };
                self.reveal(i, cx);
                self.user_select_export(i, cx);
            }
            "run" => {
                let c = Cmd::ALL.iter().copied().find(|c| c.id() == arg).ok_or_else(|| format!("no command {arg}"))?;
                self.run(c, window, cx);
            }
            "view" => {
                let v = match arg {
                    "assembly" => View::Assembly,
                    "mod" => View::Mod,
                    "audio" => View::Audio,
                    "map" => View::Map,
                    _ => View::Packages,
                };
                self.set_view(v, window, cx);
            }
            "tab" => {
                self.tab = match arg {
                    "refs" => inspector::Tab::References,
                    "source" => inspector::Tab::Source,
                    _ => inspector::Tab::Properties,
                };
                self.load_tab(cx);
            }
            "palette" => self.open_palette(arg, window, cx),
            "query" => {
                let state = self.palette.as_ref().map(|p| p.state.clone()).ok_or("no palette")?;
                let q = arg.to_string();
                let me = cx.entity();
                window.defer(cx, move |window, cx| {
                    state.update(cx, |st, cx| st.set_query(q.clone(), window, cx));
                    me.update(cx, |s, cx| s.palette_query(q, cx));
                });
            }
            "pick" => {
                let row = arg.parse::<usize>().map_err(|_| "pick <row>")?;
                self.palette_confirm(row, window, cx);
            }
            "set" => {
                let (p, v) = arg.split_once('=').ok_or("set <prop>=<value>")?;
                self.set_property(p.to_string(), v.to_string(), cx);
            }
            "meshgrid" | "meshskel" => {
                let grid = verb == "meshgrid";
                let views = [self.mesh.clone(), self.anim_view()];
                for m in views.into_iter().flatten() {
                    let mut st = m.borrow_mut();
                    if grid {
                        st.view.show_grid = !st.view.show_grid;
                    } else {
                        st.view.show_skeleton = !st.view.show_skeleton;
                    }
                    st.dirty = true;
                }
            }
            "meshwire" => {
                if let Some(m) = self.mesh.clone() {
                    m.borrow_mut().toggle_wireframe();
                }
            }
            "meshlod" => {
                let lod = arg.parse::<usize>().map_err(|_| "meshlod <n>")?;
                let (path, i) = (self.open.as_ref().map(|t| t.path.clone()).unwrap_or_default(), self.selected_export.ok_or("nothing selected")?);
                self.load_mesh(path, i, lod, cx);
            }
            "project" => {
                let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
                match verb {
                    "new" => {
                        let (loc, name) = rest.split_once(' ').ok_or("project new <folder> <name>")?;
                        let p = project::Project { file: String::new(), name: name.to_string(), id: project::id_of(name), author: "test".into(), version: "1.0.0".into(), files: Vec::new(), cooker: serde_json::Value::Null };
                        self.create_project(p, std::path::PathBuf::from(loc), window, cx);
                    }
                    "open" => self.open_project(rest.to_string(), cx),
                    "close" => self.close_project_now(true, None, cx),
                    "save" => self.save_project(cx),
                    _ => return Err("project new|open|close|save".into()),
                }
            }
            "wizard" => {
                let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
                match verb {
                    "open" => {
                        let kind = wizard::Kind::from_name(rest.trim());
                        self.open_wizard(kind, None, window, cx);
                    }
                    "close" => self.wizard = None,
                    _ => {
                        let w = self.wizard.clone().ok_or("no wizard")?;
                        let (verb, rest) = (verb.to_string(), rest.to_string());
                        window.defer(cx, move |window, cx| {
                            w.update(cx, |w, cx| w.drive(&verb, &rest, window, cx));
                        });
                    }
                }
            }
            "audio" => {
                self.set_view(View::Audio, window, cx);
                let path = self.audio.as_ref().and_then(|a| a.package_path(arg)).ok_or_else(|| format!("no sound file {arg} (yet)"))?;
                self.audio_open(path, cx);
            }
            "twrk-save" => self.save_workspace_to(false, Some(arg.to_string()), cx),
            "twrk-open" => self.open_workspace_file(arg.to_string(), cx),
            "sound" => return self.drive_sound(arg, cx),
            "anim" => return self.drive_anim(arg, cx),
            "particles" => return self.drive_particles(arg, cx),
            "movie" => {
                let m = self.movie_editor(window, cx).ok_or("no movie is shown")?;
                let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
                let (verb, rest) = (verb.to_string(), rest.to_string());
                return m.update(cx, |m, cx| m.drive(&verb, &rest, window, cx));
            }
            "material" => return self.drive_material(arg, window, cx),
            "dialog" => return dialogs::drive(arg, window, cx),
            _ => return Err(format!("unknown step {verb}")),
        }
        cx.notify();
        Ok(())
    }

    pub(super) fn bind_actions(&mut self, root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(|this, _: &NewTab, _, cx| this.new_tab(None, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| this.close_tab(this.active_tab, cx)))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.cycle_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| this.cycle_tab(-1, cx)))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(|this, _: &Up, window, cx| this.list_step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &Down, window, cx| this.list_step(1, window, cx)))
            .on_action(cx.listener(|this, _: &First, window, cx| this.list_step(isize::MIN, window, cx)))
            .on_action(cx.listener(|this, _: &Last, window, cx| this.list_step(isize::MAX, window, cx)))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.on_left(cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.on_right(cx)))
            .on_action(cx.listener(|this, _: &Commands, window, cx| this.run(Cmd::Commands, window, cx)))
            .on_action(cx.listener(|this, _: &GoToPackage, window, cx| this.run(Cmd::GoToPackage, window, cx)))
            .on_action(cx.listener(|this, _: &GoToObject, window, cx| this.run(Cmd::GoToObject, window, cx)))
            .on_action(cx.listener(|this, _: &SearchAll, window, cx| this.run(Cmd::SearchAll, window, cx)))
            .on_action(cx.listener(|this, _: &Shortcuts, window, cx| this.run(Cmd::Shortcuts, window, cx)))
            .on_action(cx.listener(|this, _: &Settings, window, cx| this.run(Cmd::Settings, window, cx)))
            .on_action(cx.listener(|this, _: &Create, window, cx| this.run(Cmd::Create, window, cx)))
            .on_action(cx.listener(|this, _: &NewWindow, window, cx| this.run(Cmd::NewWindow, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSide, _, cx| this.toggle_pane(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleSecond, _, cx| this.toggle_pane(true, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(0.1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(-0.1, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.zoom_by(0.0, cx)))
            .on_action(cx.listener(|this, _: &CopyPath, window, cx| this.run(Cmd::CopyPath, window, cx)))
            .on_action(cx.listener(|this, _: &ShowPackages, window, cx| this.set_view(View::Packages, window, cx)))
            .on_action(cx.listener(|this, _: &ShowAssembly, window, cx| this.set_view(View::Assembly, window, cx)))
            .on_action(cx.listener(|this, _: &ShowMod, window, cx| this.set_view(View::Mod, window, cx)))
            .on_action(cx.listener(|this, _: &ShowAudio, window, cx| this.set_view(View::Audio, window, cx)))
            .on_action(cx.listener(|this, _: &ShowMap, window, cx| this.set_view(View::Map, window, cx)))
            .on_action(cx.listener(|this, _: &Rename, window, cx| this.run(Cmd::Rename, window, cx)))
            .on_action(cx.listener(|this, _: &SaveWorkspace, window, cx| this.run(Cmd::SaveWorkspace, window, cx)))
            .on_action(cx.listener(|this, _: &OpenWorkspaceFile, window, cx| this.run(Cmd::OpenWorkspaceFile, window, cx)))
            .on_action(cx.listener(|this, _: &Duplicate, window, cx| this.run(Cmd::Duplicate, window, cx)))
            .on_action(cx.listener(|this, _: &Delete, window, cx| this.run(Cmd::Delete, window, cx)))
    }
}

pub fn bind_keys(cx: &mut App) {
    let m = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    let k = |keys: &str| -> &'static str { Box::leak(keys.replace("mod", m).into_boxed_str()) };
    cx.bind_keys([
        KeyBinding::new(k("mod-z"), Undo, Some("Studio")),
        KeyBinding::new(k("mod-s"), Save, Some("Studio")),
        KeyBinding::new(k("mod-k"), Commands, Some("Studio")),
        KeyBinding::new(k("mod-shift-p"), Commands, Some("Studio")),
        KeyBinding::new(k("mod-p"), GoToPackage, Some("Studio")),
        KeyBinding::new(k("mod-shift-o"), GoToObject, Some("Studio")),
        KeyBinding::new(k("mod-shift-f"), SearchAll, Some("Studio")),
        KeyBinding::new(k("mod-t"), NewTab, Some("Studio")),
        KeyBinding::new(k("mod-w"), CloseTab, Some("Studio")),
        KeyBinding::new(k("mod-1"), ShowPackages, Some("Studio")),
        KeyBinding::new(k("mod-2"), ShowAssembly, Some("Studio")),
        KeyBinding::new(k("mod-3"), ShowMod, Some("Studio")),
        KeyBinding::new(k("mod-4"), ShowAudio, Some("Studio")),
        KeyBinding::new(k("mod-5"), ShowMap, Some("Studio")),
        KeyBinding::new(k("mod-d"), Duplicate, Some("Exports")),
        KeyBinding::new(k("mod-alt-s"), SaveWorkspace, Some("Studio")),
        KeyBinding::new(k("mod-alt-o"), OpenWorkspaceFile, Some("Studio")),
        KeyBinding::new("f1", Shortcuts, Some("Studio")),
        KeyBinding::new(k("mod-,"), Settings, Some("Studio")),
        KeyBinding::new(k("mod-n"), Create, Some("Studio")),
        KeyBinding::new(k("mod-shift-n"), NewWindow, Some("Studio")),
        KeyBinding::new(k("mod-b"), ToggleSide, Some("Studio")),
        KeyBinding::new(k("mod-j"), ToggleSecond, Some("Studio")),
        KeyBinding::new(k("mod-="), ZoomIn, Some("Studio")),
        KeyBinding::new(k("mod-+"), ZoomIn, Some("Studio")),
        KeyBinding::new(k("mod--"), ZoomOut, Some("Studio")),
        KeyBinding::new(k("mod-0"), ZoomReset, Some("Studio")),
        KeyBinding::new(k("mod-shift-c"), CopyPath, Some("Exports")),
        KeyBinding::new("f2", Rename, Some("Exports")),
        KeyBinding::new("backspace", Delete, Some("Exports")),
        KeyBinding::new("delete", Delete, Some("Exports")),
        KeyBinding::new("ctrl-tab", NextTab, Some("Studio")),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some("Studio")),
        KeyBinding::new("alt-left", Back, Some("Studio")),
        KeyBinding::new("alt-right", Forward, Some("Studio")),
        KeyBinding::new("up", Up, Some("Packages")),
        KeyBinding::new("down", Down, Some("Packages")),
        KeyBinding::new("home", First, Some("Packages")),
        KeyBinding::new("end", Last, Some("Packages")),
        KeyBinding::new("up", Up, Some("Exports")),
        KeyBinding::new("down", Down, Some("Exports")),
        KeyBinding::new("home", First, Some("Exports")),
        KeyBinding::new("end", Last, Some("Exports")),
        KeyBinding::new("left", Left, Some("Exports")),
        KeyBinding::new("right", Right, Some("Exports")),
        KeyBinding::new(k("mod-c"), CopyRows, Some("Assembly")),
        KeyBinding::new(k("mod-v"), PasteRows, Some("Assembly")),
        KeyBinding::new("up", Up, Some("Assembly")),
        KeyBinding::new("down", Down, Some("Assembly")),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([KeyBinding::new("cmd-[", Back, Some("Studio")), KeyBinding::new("cmd-]", Forward, Some("Studio"))]);
    }
}
